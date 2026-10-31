//! Mock-gateway tests for the surface added to stay in lockstep with the
//! rust-gvm-api contract (121 paths / 199 operations): every new tool is
//! exercised for its exact method, path, query and body shape so a drift
//! between this server and the gateway spec fails here, not in production.

mod support;

use std::collections::BTreeMap;

use gvm_mcp::mcp::GvmMcpServer;
use gvm_mcp::mcp::tools::agents::{
    AgentInstallerInstructionParams, AgentSupportBundleParams, CreateAgentGroupParams,
    UpdateAgentControlScanConfigParams, UpdateAgentGroupParams, UpdateAgentParams,
};
use gvm_mcp::mcp::tools::assets::{
    AssetByIdParams, CreateHostParams, ListAssetsParams, UpdateAssetParams, UpdateCommentParams,
};
use gvm_mcp::mcp::tools::common::{
    DeleteParams, FilterParams, GetByIdParams, ListParams, PageParams, TrashListParams,
};
use gvm_mcp::mcp::tools::compliance::{CreatePolicyParams, UpdatePolicyParams};
use gvm_mcp::mcp::tools::credentials::{
    CreateCredentialParams, CredentialStorePreference, UpdateCredentialStoreParams,
};
use gvm_mcp::mcp::tools::feeds::ListFeedsParams;
use gvm_mcp::mcp::tools::filters::{CreateFilterParams, UpdateFilterParams};
use gvm_mcp::mcp::tools::nvts::SearchNvtsParams;
use gvm_mcp::mcp::tools::port_lists::UpdatePortListParams;
use gvm_mcp::mcp::tools::reports::{GetReportParams, ImportReportParams, ReportPageParams};
use gvm_mcp::mcp::tools::scan_configs::{
    CreateScanConfigParams, FamilySelection, ListConfigsParams, ScanConfigNvtParams,
    ScanConfigNvtsParams, ScanConfigPreferenceParams, ScanConfigPreferencesParams,
    SetFamilySelectionParams, SetNvtSelectionParams, SetScanConfigPreferenceParams,
};
use gvm_mcp::mcp::tools::secinfo::SecInfoIdParams;
use gvm_mcp::mcp::tools::tags::{CreateTagParams, UpdateTagParams};
use gvm_mcp::mcp::tools::targets::{
    CreateOciImageTargetParams, CreateWebApplicationTargetParams, UpdateOciImageTargetParams,
    UpdateWebApplicationTargetParams,
};
use gvm_mcp::mcp::tools::tasks::{CreateTaskParams, UpdateTaskParams};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use support::config_for;
use wiremock::matchers::{body_json, header, method, path, query_param, query_param_is_missing};
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

fn assert_ok(result: &CallToolResult) -> serde_json::Value {
    assert_ne!(result.is_error, Some(true), "tool errored: {result:?}");
    json_of(result)
}

async fn server() -> (MockServer, GvmMcpServer) {
    let server = MockServer::start().await;
    let mcp = GvmMcpServer::new(config_for(&server)).unwrap();
    (server, mcp)
}

fn list_body(row: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "data": [row],
        "pagination": {"page": 1, "perPage": 25, "total": 1, "totalPages": 1}
    })
}

fn created() -> ResponseTemplate {
    ResponseTemplate::new(201).set_body_json(serde_json::json!({"id": "new-id"}))
}

fn by_id() -> GetByIdParams {
    GetByIdParams { id: "x-1".into() }
}

fn ultimate() -> DeleteParams {
    DeleteParams {
        id: "x-1".into(),
        ultimate: Some(true),
    }
}

// ───── clone endpoints (targets, tasks, filters, tags, configs, agent groups, specialized targets)

#[tokio::test]
async fn clone_tools_post_to_clone_subresource() {
    let (server, mcp) = server().await;
    for resource in [
        "targets",
        "tasks",
        "filters",
        "tags",
        "configs",
        "agent-groups",
        "oci-image-targets",
        "web-application-targets",
    ] {
        Mock::given(method("POST"))
            .and(path(format!("/api/v1/{resource}/x-1/clone")))
            .respond_with(created())
            .expect(1)
            .mount(&server)
            .await;
    }

    for result in [
        mcp.clone_target(Parameters(by_id())).await.unwrap(),
        mcp.clone_task(Parameters(by_id())).await.unwrap(),
        mcp.clone_filter(Parameters(by_id())).await.unwrap(),
        mcp.clone_tag(Parameters(by_id())).await.unwrap(),
        mcp.clone_config(Parameters(by_id())).await.unwrap(),
        mcp.clone_agent_group(Parameters(by_id())).await.unwrap(),
        mcp.clone_oci_image_target(Parameters(by_id()))
            .await
            .unwrap(),
        mcp.clone_web_application_target(Parameters(by_id()))
            .await
            .unwrap(),
    ] {
        assert_eq!(assert_ok(&result)["id"], "new-id");
    }
}

// ───── system

#[tokio::test]
async fn timezones_passthrough() {
    let (server, mcp) = server().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/timezones"))
        .and(header("authorization", support::EXPECTED_BASIC))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{"name": "Europe/Istanbul", "offset": "+03:00"}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let json = assert_ok(&mcp.list_timezones().await.unwrap());
    assert_eq!(json["data"][0]["name"], "Europe/Istanbul");
}

// ───── tasks: every variant, extended update

#[tokio::test]
async fn create_task_sends_variant_selectors_and_preferences() {
    let (server, mcp) = server().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/tasks"))
        .and(body_json(serde_json::json!({
            "name": "crawl shop",
            "type": "webApplication",
            "webApplicationTargetId": "wat-1",
            "scannerId": "s-1",
            "observers": ["alice"],
            "schedulePeriods": 3,
            "preferences": {"max_checks": "4"}
        })))
        .respond_with(created())
        .expect(1)
        .mount(&server)
        .await;

    let result = mcp
        .create_task(Parameters(CreateTaskParams {
            name: "crawl shop".into(),
            comment: None,
            task_type: Some("webApplication".into()),
            target_id: None,
            agent_group_id: None,
            oci_image_target_id: None,
            web_application_target_id: Some("wat-1".into()),
            scan_config_id: None,
            scanner_id: Some("s-1".into()),
            schedule_id: None,
            alert_ids: None,
            alterable: None,
            observers: Some(vec!["alice".into()]),
            schedule_periods: Some(3),
            preferences: Some(BTreeMap::from([(
                "max_checks".to_string(),
                "4".to_string(),
            )])),
        }))
        .await
        .unwrap();
    assert_eq!(assert_ok(&result)["id"], "new-id");
}

#[tokio::test]
async fn create_import_task_sends_only_type_name_comment() {
    let (server, mcp) = server().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/tasks"))
        .and(body_json(
            serde_json::json!({"name": "imports", "type": "import"}),
        ))
        .respond_with(created())
        .expect(1)
        .mount(&server)
        .await;
    let result = mcp
        .create_task(Parameters(CreateTaskParams {
            name: "imports".into(),
            comment: None,
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
        .unwrap();
    assert_eq!(assert_ok(&result)["id"], "new-id");
}

#[tokio::test]
async fn update_task_sends_alterable_observers_preferences() {
    let (server, mcp) = server().await;
    Mock::given(method("PUT"))
        .and(path("/api/v1/tasks/t-1"))
        .and(body_json(serde_json::json!({
            "alterable": true,
            "observers": ["bob"],
            "preferences": {"auto_delete": "keep"}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "t-1", "name": "weekly", "status": "New"
        })))
        .expect(1)
        .mount(&server)
        .await;
    let result = mcp
        .update_task(Parameters(UpdateTaskParams {
            id: "t-1".into(),
            name: None,
            comment: None,
            target_id: None,
            scan_config_id: None,
            scanner_id: None,
            schedule_id: None,
            alert_ids: None,
            alterable: Some(true),
            observers: Some(vec!["bob".into()]),
            schedule_periods: None,
            preferences: Some(BTreeMap::from([(
                "auto_delete".to_string(),
                "keep".to_string(),
            )])),
        }))
        .await
        .unwrap();
    assert_eq!(assert_ok(&result)["id"], "t-1");
}

// ───── compliance

#[tokio::test]
async fn audit_lifecycle_hits_audit_routes() {
    let (server, mcp) = server().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/audits"))
        .and(query_param("filter", "name~pci"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(list_body(serde_json::json!({
                "id": "a-1", "name": "pci audit", "status": "Done", "usageType": "audit",
                "target": {"id": "tg-1", "name": "cardholder"}, "reportCount": 1,
                "lastReport": {"id": "r-1"}, "writable": true
            }))),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/audits/a-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "a-1", "name": "pci audit", "status": "Done"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/audits"))
        .and(body_json(serde_json::json!({
            "name": "pci audit", "targetId": "tg-1", "scanConfigId": "pol-1", "scannerId": "s-1"
        })))
        .respond_with(created())
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/api/v1/audits/a-1"))
        .and(body_json(serde_json::json!({"comment": "quarterly"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "a-1", "name": "pci audit", "status": "New", "comment": "quarterly"
        })))
        .expect(1)
        .mount(&server)
        .await;
    for action in ["start", "resume"] {
        Mock::given(method("POST"))
            .and(path(format!("/api/v1/audits/a-1/{action}")))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"reportId": "r-9"})),
            )
            .expect(1)
            .mount(&server)
            .await;
    }
    Mock::given(method("POST"))
        .and(path("/api/v1/audits/a-1/stop"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    // Audits have no `ultimate` option.
    Mock::given(method("DELETE"))
        .and(path("/api/v1/audits/a-1"))
        .and(query_param_is_missing("ultimate"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let list = assert_ok(
        &mcp.list_audits(Parameters(ListParams {
            filter: Some("name~pci".into()),
            ..Default::default()
        }))
        .await
        .unwrap(),
    );
    assert_eq!(list["audits"][0]["usageType"], "audit");
    assert_eq!(list["audits"][0]["target"], "cardholder");
    assert!(list["audits"][0].get("writable").is_none());

    let one = GetByIdParams { id: "a-1".into() };
    assert_eq!(
        assert_ok(&mcp.get_audit(Parameters(one)).await.unwrap())["id"],
        "a-1"
    );
    let created_audit = mcp
        .create_audit(Parameters(CreateTaskParams {
            name: "pci audit".into(),
            comment: None,
            task_type: None,
            target_id: Some("tg-1".into()),
            agent_group_id: None,
            oci_image_target_id: None,
            web_application_target_id: None,
            scan_config_id: Some("pol-1".into()),
            scanner_id: Some("s-1".into()),
            schedule_id: None,
            alert_ids: None,
            alterable: None,
            observers: None,
            schedule_periods: None,
            preferences: None,
        }))
        .await
        .unwrap();
    assert_eq!(assert_ok(&created_audit)["id"], "new-id");
    let updated = mcp
        .update_audit(Parameters(UpdateTaskParams {
            id: "a-1".into(),
            name: None,
            comment: Some("quarterly".into()),
            target_id: None,
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
        .unwrap();
    assert_eq!(assert_ok(&updated)["comment"], "quarterly");
    let id = || GetByIdParams { id: "a-1".into() };
    assert_eq!(
        assert_ok(&mcp.start_audit(Parameters(id())).await.unwrap())["reportId"],
        "r-9"
    );
    assert_eq!(
        assert_ok(&mcp.stop_audit(Parameters(id())).await.unwrap())["stopped"],
        true
    );
    assert_eq!(
        assert_ok(&mcp.resume_audit(Parameters(id())).await.unwrap())["reportId"],
        "r-9"
    );
    assert_eq!(
        assert_ok(&mcp.delete_audit(Parameters(id())).await.unwrap())["deleted"],
        true
    );
}

#[tokio::test]
async fn policy_crud_hits_policy_routes() {
    let (server, mcp) = server().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/policies"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(list_body(serde_json::json!({
                "id": "pol-1", "name": "CIS", "usageType": "policy", "familyCount": 3,
                "nvtCount": 40, "inUse": false, "writable": true
            }))),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/policies/pol-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "pol-1", "name": "CIS", "usageType": "policy"
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/policies"))
        .and(body_json(serde_json::json!({
            "name": "CIS copy", "basePolicyId": "pol-1", "comment": "tuned"
        })))
        .respond_with(created())
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/api/v1/policies/pol-1"))
        .and(body_json(serde_json::json!({"name": "CIS v2"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "pol-1", "name": "CIS v2"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/policies/pol-1"))
        .and(query_param_is_missing("ultimate"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let list = assert_ok(
        &mcp.list_policies(Parameters(ListParams::default()))
            .await
            .unwrap(),
    );
    assert_eq!(list["policies"][0]["usageType"], "policy");
    assert!(list["policies"][0].get("writable").is_none());
    let id = || GetByIdParams { id: "pol-1".into() };
    assert_eq!(
        assert_ok(&mcp.get_policy(Parameters(id())).await.unwrap())["name"],
        "CIS"
    );
    let created_policy = mcp
        .create_policy(Parameters(CreatePolicyParams {
            name: "CIS copy".into(),
            base_policy_id: "pol-1".into(),
            comment: Some("tuned".into()),
        }))
        .await
        .unwrap();
    assert_eq!(assert_ok(&created_policy)["id"], "new-id");
    let updated = mcp
        .update_policy(Parameters(UpdatePolicyParams {
            id: "pol-1".into(),
            name: Some("CIS v2".into()),
            comment: None,
        }))
        .await
        .unwrap();
    assert_eq!(assert_ok(&updated)["name"], "CIS v2");
    assert_eq!(
        assert_ok(&mcp.delete_policy(Parameters(id())).await.unwrap())["deleted"],
        true
    );
}

// ───── scan configs: required base, sub-resources, generic configs

#[tokio::test]
async fn create_scan_config_always_sends_base_id() {
    let (server, mcp) = server().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/scan-configs"))
        .and(body_json(
            serde_json::json!({"name": "tuned", "baseScanConfigId": "sc-base"}),
        ))
        .respond_with(created())
        .expect(1)
        .mount(&server)
        .await;
    let result = mcp
        .create_scan_config(Parameters(CreateScanConfigParams {
            name: "tuned".into(),
            base_scan_config_id: "sc-base".into(),
            comment: None,
        }))
        .await
        .unwrap();
    assert_eq!(assert_ok(&result)["id"], "new-id");
}

#[tokio::test]
async fn scan_config_subresources_use_spec_paths_and_bodies() {
    let (server, mcp) = server().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/scan-configs/sc-1/nvts"))
        .and(query_param("family", "Web Servers"))
        .and(query_param("perPage", "5"))
        .and(query_param_is_missing("filter"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(list_body(serde_json::json!({
                "oid": "1.3.6.1.4.1.25623.1.0.1", "name": "Apache", "family": "Web Servers",
                "severity": 5.0, "tags": "dropped"
            }))),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(
            "/api/v1/scan-configs/sc-1/nvts/1.3.6.1.4.1.25623.1.0.1",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "oid": "1.3.6.1.4.1.25623.1.0.1", "name": "Apache", "tags": "kept"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/scan-configs/sc-1/preferences"))
        .and(query_param("nvtOid", "1.3.6.1.4.1.25623.1.0.1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{"name": "Timeout", "value": "60", "nvt": {"oid": "1.3.6.1.4.1.25623.1.0.1"}}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/scan-configs/sc-1/preferences/Timeout"))
        .and(query_param_is_missing("nvtOid"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "Timeout", "value": "60"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/api/v1/scan-configs/sc-1/preferences/Timeout"))
        .and(body_json(
            serde_json::json!({"nvtOid": "1.3.6.1.4.1.25623.1.0.1", "value": "120"}),
        ))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/api/v1/scan-configs/sc-1/family-selection"))
        .and(body_json(serde_json::json!({
            "families": [{"name": "Web Servers", "growing": true, "all": false}],
            "autoAddNewFamilies": false
        })))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        // Family names travel as one percent-encoded path segment.
        .and(path(
            "/api/v1/scan-configs/sc-1/families/Web%20Servers/nvts",
        ))
        .and(body_json(
            serde_json::json!({"nvtOids": ["1.3.6.1.4.1.25623.1.0.1"]}),
        ))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let nvts = assert_ok(
        &mcp.list_scan_config_nvts(Parameters(ScanConfigNvtsParams {
            id: "sc-1".into(),
            family: Some("Web Servers".into()),
            page: PageParams {
                page: None,
                per_page: Some(5),
            },
        }))
        .await
        .unwrap(),
    );
    assert_eq!(nvts["nvts"][0]["family"], "Web Servers");
    assert!(nvts["nvts"][0].get("tags").is_none());

    let nvt = assert_ok(
        &mcp.get_scan_config_nvt(Parameters(ScanConfigNvtParams {
            id: "sc-1".into(),
            oid: "1.3.6.1.4.1.25623.1.0.1".into(),
        }))
        .await
        .unwrap(),
    );
    assert_eq!(nvt["tags"], "kept");

    let prefs = assert_ok(
        &mcp.list_scan_config_preferences(Parameters(ScanConfigPreferencesParams {
            id: "sc-1".into(),
            nvt_oid: Some("1.3.6.1.4.1.25623.1.0.1".into()),
        }))
        .await
        .unwrap(),
    );
    assert_eq!(prefs["data"][0]["name"], "Timeout");

    let pref = assert_ok(
        &mcp.get_scan_config_preference(Parameters(ScanConfigPreferenceParams {
            id: "sc-1".into(),
            name: "Timeout".into(),
            nvt_oid: None,
        }))
        .await
        .unwrap(),
    );
    assert_eq!(pref["value"], "60");

    let set = assert_ok(
        &mcp.set_scan_config_preference(Parameters(SetScanConfigPreferenceParams {
            id: "sc-1".into(),
            name: "Timeout".into(),
            nvt_oid: Some("1.3.6.1.4.1.25623.1.0.1".into()),
            value: Some("120".into()),
        }))
        .await
        .unwrap(),
    );
    assert_eq!(set["updated"], true);

    let families = assert_ok(
        &mcp.set_scan_config_family_selection(Parameters(SetFamilySelectionParams {
            id: "sc-1".into(),
            families: vec![FamilySelection {
                name: "Web Servers".into(),
                growing: true,
                all: false,
            }],
            auto_add_new_families: Some(false),
        }))
        .await
        .unwrap(),
    );
    assert_eq!(families["updated"], true);

    let selection = assert_ok(
        &mcp.set_scan_config_nvt_selection(Parameters(SetNvtSelectionParams {
            id: "sc-1".into(),
            family: "Web Servers".into(),
            nvt_oids: vec!["1.3.6.1.4.1.25623.1.0.1".into()],
        }))
        .await
        .unwrap(),
    );
    assert_eq!(selection["updated"], true);
}

#[tokio::test]
async fn generic_configs_pass_usage_type_and_support_delete() {
    let (server, mcp) = server().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/configs"))
        .and(query_param("usageType", "policy"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(list_body(serde_json::json!({
                "id": "c-1", "name": "CIS", "usageType": "policy", "type": 0, "inUse": false,
                "writable": true
            }))),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/configs/c-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "c-1", "name": "CIS", "usageType": "policy"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/configs/x-1"))
        .and(query_param("ultimate", "true"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let list = assert_ok(
        &mcp.list_configs(Parameters(ListConfigsParams {
            list: ListParams::default(),
            usage_type: Some("policy".into()),
        }))
        .await
        .unwrap(),
    );
    assert_eq!(list["configs"][0]["usageType"], "policy");
    assert_eq!(
        assert_ok(
            &mcp.get_config(Parameters(GetByIdParams { id: "c-1".into() }))
                .await
                .unwrap()
        )["name"],
        "CIS"
    );
    assert_eq!(
        assert_ok(&mcp.delete_config(Parameters(ultimate())).await.unwrap())["deleted"],
        true
    );
}

// ───── credentials: store-backed fields + store administration

#[tokio::test]
async fn store_backed_credential_sends_store_fields() {
    let (server, mcp) = server().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/credentials"))
        .and(body_json(serde_json::json!({
            "name": "vault-ssh", "type": "cs_up", "credentialStoreId": "store-1",
            "vaultId": "kv/ssh", "hostIdentifier": "web01"
        })))
        .respond_with(created())
        .expect(1)
        .mount(&server)
        .await;
    let result = mcp
        .create_credential(Parameters(CreateCredentialParams {
            name: "vault-ssh".into(),
            credential_type: "cs_up".into(),
            comment: None,
            login: None,
            password: None,
            private_key: None,
            certificate: None,
            community: None,
            auth_algorithm: None,
            privacy_algorithm: None,
            privacy_password: None,
            credential_store_id: Some("store-1".into()),
            vault_id: Some("kv/ssh".into()),
            host_identifier: Some("web01".into()),
        }))
        .await
        .unwrap();
    assert_eq!(assert_ok(&result)["id"], "new-id");
}

#[tokio::test]
async fn credential_store_admin_routes() {
    let (server, mcp) = server().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/credential-stores"))
        .and(query_param_is_missing("page"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{"id": "store-1", "name": "vault", "writable": true}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/credential-stores/store-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "store-1", "name": "vault", "host": "vault.example"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/api/v1/credential-stores/store-1"))
        .and(body_json(serde_json::json!({
            "active": true, "host": "vault.example", "port": 8200,
            "preferences": [{"name": "token", "value": "s3cret"}]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "store-1", "name": "vault", "host": "vault.example"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/credential-stores/store-1/actions/verify"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let stores = assert_ok(&mcp.list_credential_stores().await.unwrap());
    assert_eq!(stores["credentialStores"][0]["id"], "store-1");
    let id = || GetByIdParams {
        id: "store-1".into(),
    };
    assert_eq!(
        assert_ok(&mcp.get_credential_store(Parameters(id())).await.unwrap())["host"],
        "vault.example"
    );
    let updated = mcp
        .update_credential_store(Parameters(UpdateCredentialStoreParams {
            id: "store-1".into(),
            active: Some(true),
            host: Some("vault.example".into()),
            path: None,
            port: Some(8200),
            comment: None,
            preferences: Some(vec![CredentialStorePreference {
                name: "token".into(),
                value: "s3cret".into(),
            }]),
        }))
        .await
        .unwrap();
    assert_eq!(assert_ok(&updated)["id"], "store-1");
    assert_eq!(
        assert_ok(&mcp.verify_credential_store(Parameters(id())).await.unwrap())["verified"],
        true
    );
}

// ───── reports: new pages, paged detail, import

#[tokio::test]
async fn report_pages_keep_their_spec_row_keys() {
    let (server, mcp) = server().await;
    let pages: [(&str, serde_json::Value, &str, &str); 8] = [
        (
            "hosts",
            serde_json::json!({"id": "h", "name": "10.0.0.5", "severity": 7.5, "extra": 1}),
            "hosts",
            "name",
        ),
        (
            "ports",
            serde_json::json!({"id": "p", "name": "443/tcp", "severity": 5.0}),
            "ports",
            "name",
        ),
        (
            "applications",
            serde_json::json!({"id": "cpe:/a:x", "name": "cpe:/a:x", "severity": 5.0}),
            "applications",
            "name",
        ),
        (
            "cves",
            serde_json::json!({"id": "CVE-1", "name": "CVE-1", "severity": 9.8}),
            "cves",
            "name",
        ),
        (
            "operating-systems",
            serde_json::json!({"bestOsCpe": "cpe:/o:linux", "bestOsText": "Linux", "hostsCount": 2}),
            "operatingSystems",
            "bestOsCpe",
        ),
        (
            "closed-cves",
            serde_json::json!({"id": "c", "cve": "CVE-2", "nvt": {"oid": "1.2"}, "host": "10.0.0.5", "severity": 4.0, "threat": "Medium"}),
            "closedCves",
            "cve",
        ),
        (
            "errors",
            serde_json::json!({"id": "e", "name": "err", "nvtName": "Timeout NVT", "host": "10.0.0.5", "port": "general/tcp", "description": "timed out"}),
            "errors",
            "nvtName",
        ),
        (
            "vulnerabilities",
            serde_json::json!({"id": "v", "nvt": {"oid": "1.2", "name": "OpenSSH"}, "host": "10.0.0.5", "hostsCount": 1, "port": "22/tcp", "severity": 7.5, "threat": "High", "occurrences": 2}),
            "vulnerabilities",
            "nvt",
        ),
    ];
    for (page, row, _, _) in &pages {
        Mock::given(method("GET"))
            .and(path(format!("/api/v1/reports/rep-1/{page}")))
            .and(query_param("page", "2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(list_body(row.clone())))
            .expect(1)
            .mount(&server)
            .await;
    }
    let params = || ReportPageParams {
        id: "rep-1".into(),
        list: ListParams {
            page: Some(2),
            ..Default::default()
        },
    };
    let results = [
        mcp.get_report_hosts(Parameters(params())).await.unwrap(),
        mcp.get_report_ports(Parameters(params())).await.unwrap(),
        mcp.get_report_applications(Parameters(params()))
            .await
            .unwrap(),
        mcp.get_report_cves(Parameters(params())).await.unwrap(),
        mcp.get_report_operating_systems(Parameters(params()))
            .await
            .unwrap(),
        mcp.get_report_closed_cves(Parameters(params()))
            .await
            .unwrap(),
        mcp.get_report_errors(Parameters(params())).await.unwrap(),
        mcp.get_report_vulnerabilities(Parameters(params()))
            .await
            .unwrap(),
    ];
    for ((_, _, out_key, kept_key), result) in pages.iter().zip(results.iter()) {
        let json = assert_ok(result);
        assert!(
            !json[*out_key][0][*kept_key].is_null(),
            "{out_key} row lost {kept_key}: {json}"
        );
        assert_eq!(json["pagination"]["total"], 1);
    }
    // The hosts row must have dropped the unknown `extra` key.
    assert!(assert_ok(&results[0])["hosts"][0].get("extra").is_none());
}

#[tokio::test]
async fn get_report_forwards_embedded_result_paging() {
    let (server, mcp) = server().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/reports/rep-1"))
        .and(query_param("page", "3"))
        .and(query_param("perPage", "10"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "rep-1", "results": []
        })))
        .expect(1)
        .mount(&server)
        .await;
    let json = assert_ok(
        &mcp.get_report(Parameters(GetReportParams {
            id: "rep-1".into(),
            page: Some(3),
            per_page: Some(10),
        }))
        .await
        .unwrap(),
    );
    assert_eq!(json["id"], "rep-1");
}

#[tokio::test]
async fn import_report_posts_xml_with_task_query() {
    let (server, mcp) = server().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/reports"))
        .and(query_param("taskId", "t-import"))
        .and(query_param("inAssets", "false"))
        .and(header("content-type", "application/xml"))
        .and(wiremock::matchers::body_string("<report id=\"r\"/>"))
        .respond_with(created())
        .expect(1)
        .mount(&server)
        .await;
    let json = assert_ok(
        &mcp.import_report(Parameters(ImportReportParams {
            task_id: "t-import".into(),
            report_xml: "<report id=\"r\"/>".into(),
            in_assets: Some(false),
        }))
        .await
        .unwrap(),
    );
    assert_eq!(json["id"], "new-id");
}

#[tokio::test]
async fn import_report_rejects_empty_body_locally() {
    let (_server, mcp) = server().await;
    let result = mcp
        .import_report(Parameters(ImportReportParams {
            task_id: "t-import".into(),
            report_xml: String::new(),
            in_assets: None,
        }))
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
}

// ───── assets: hosts writes, operating systems, generic assets

#[tokio::test]
async fn host_and_os_asset_writes() {
    let (server, mcp) = server().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/hosts"))
        .and(body_json(
            serde_json::json!({"value": "10.0.0.9", "comment": "lab"}),
        ))
        .respond_with(created())
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/api/v1/hosts/h-1"))
        .and(body_json(serde_json::json!({"comment": "renamed"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": "h-1"})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/hosts/x-1"))
        .and(query_param_is_missing("ultimate"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/operating-systems"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(list_body(serde_json::json!({
                "id": "os-1", "name": "cpe:/o:linux", "title": "Linux", "hostsCount": 3,
                "latestSeverity": "7.5", "writable": false
            }))),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/operating-systems/os-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": "os-1"})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/api/v1/operating-systems/os-1"))
        .and(body_json(serde_json::json!({"comment": "eol"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": "os-1"})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/operating-systems/x-1"))
        .and(query_param_is_missing("ultimate"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    assert_eq!(
        assert_ok(
            &mcp.create_asset_host(Parameters(CreateHostParams {
                value: "10.0.0.9".into(),
                comment: Some("lab".into()),
            }))
            .await
            .unwrap()
        )["id"],
        "new-id"
    );
    assert_eq!(
        assert_ok(
            &mcp.update_asset_host(Parameters(UpdateCommentParams {
                id: "h-1".into(),
                comment: Some("renamed".into()),
            }))
            .await
            .unwrap()
        )["id"],
        "h-1"
    );
    assert_eq!(
        assert_ok(&mcp.delete_asset_host(Parameters(by_id())).await.unwrap())["deleted"],
        true
    );
    let os_list = assert_ok(
        &mcp.list_operating_systems(Parameters(ListParams::default()))
            .await
            .unwrap(),
    );
    assert_eq!(os_list["operatingSystems"][0]["hostsCount"], 3);
    assert!(os_list["operatingSystems"][0].get("writable").is_none());
    assert_eq!(
        assert_ok(
            &mcp.get_operating_system(Parameters(GetByIdParams { id: "os-1".into() }))
                .await
                .unwrap()
        )["id"],
        "os-1"
    );
    assert_eq!(
        assert_ok(
            &mcp.update_operating_system(Parameters(UpdateCommentParams {
                id: "os-1".into(),
                comment: Some("eol".into()),
            }))
            .await
            .unwrap()
        )["id"],
        "os-1"
    );
    assert_eq!(
        assert_ok(
            &mcp.delete_operating_system(Parameters(by_id()))
                .await
                .unwrap()
        )["deleted"],
        true
    );
}

#[tokio::test]
async fn generic_assets_require_type_selector() {
    let (server, mcp) = server().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/assets"))
        .and(query_param("type", "tls_certificate"))
        .and(query_param("filter", "valid=1"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(list_body(serde_json::json!({
                "id": "as-1", "name": "CN=web", "type": "tls_certificate", "writable": false
            }))),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/assets/as-1"))
        .and(query_param("type", "host"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": "as-1"})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/api/v1/assets/as-1"))
        .and(query_param("type", "os"))
        .and(body_json(serde_json::json!({"comment": "note"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": "as-1"})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/assets/x-1"))
        .and(query_param_is_missing("ultimate"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let list = assert_ok(
        &mcp.list_assets(Parameters(ListAssetsParams {
            asset_type: "tls_certificate".into(),
            list: ListParams {
                filter: Some("valid=1".into()),
                ..Default::default()
            },
        }))
        .await
        .unwrap(),
    );
    assert_eq!(list["assets"][0]["type"], "tls_certificate");
    assert_eq!(
        assert_ok(
            &mcp.get_asset(Parameters(AssetByIdParams {
                id: "as-1".into(),
                asset_type: "host".into(),
            }))
            .await
            .unwrap()
        )["id"],
        "as-1"
    );
    assert_eq!(
        assert_ok(
            &mcp.update_asset(Parameters(UpdateAssetParams {
                id: "as-1".into(),
                asset_type: "os".into(),
                comment: Some("note".into()),
            }))
            .await
            .unwrap()
        )["id"],
        "as-1"
    );
    assert_eq!(
        assert_ok(&mcp.delete_asset(Parameters(by_id())).await.unwrap())["deleted"],
        true
    );
}

// ───── TLS certificates, vulnerabilities, SecInfo

#[tokio::test]
async fn tls_vulnerability_and_secinfo_catalogs() {
    let (server, mcp) = server().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/tls-certificates"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(list_body(serde_json::json!({
                "id": "tls-1", "name": "CN=web", "subjectDn": "CN=web", "issuerDn": "CN=ca",
                "sha256Fingerprint": "ab:cd", "valid": true, "certificate": "-----BEGIN"
            }))),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/tls-certificates/tls-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "tls-1", "certificate": "-----BEGIN"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/vulnerabilities"))
        .and(query_param("filter", "severity>7"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(list_body(serde_json::json!({
                "id": "1.3.6.1.4.1.25623.1.0.1", "name": "OpenSSH"
            }))),
        )
        .expect(1)
        .mount(&server)
        .await;
    // SecInfo ids may contain `/`; the client keeps each id one path
    // segment by percent-encoding it, which axum decodes on the gateway.
    for (resource, id, encoded) in [
        ("cves", "CVE-2024-3094", "CVE-2024-3094"),
        ("cpes", "cpe:/a:openbsd:openssh", "cpe:%2Fa:openbsd:openssh"),
        ("cert-bund-advisories", "CB-K24/0001", "CB-K24%2F0001"),
        (
            "dfn-cert-advisories",
            "DFN-CERT-2024-0001",
            "DFN-CERT-2024-0001",
        ),
    ] {
        Mock::given(method("GET"))
            .and(path(format!("/api/v1/{resource}")))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(list_body(serde_json::json!({
                    "id": id, "name": id
                }))),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/api/v1/{resource}/{encoded}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": id, "name": id
            })))
            .expect(1)
            .mount(&server)
            .await;
    }

    let tls = assert_ok(
        &mcp.list_tls_certificates(Parameters(ListParams::default()))
            .await
            .unwrap(),
    );
    assert_eq!(tls["tlsCertificates"][0]["valid"], true);
    assert!(tls["tlsCertificates"][0].get("certificate").is_none());
    assert_eq!(
        assert_ok(
            &mcp.get_tls_certificate(Parameters(GetByIdParams { id: "tls-1".into() }))
                .await
                .unwrap()
        )["certificate"],
        "-----BEGIN"
    );
    let vulns = assert_ok(
        &mcp.list_vulnerabilities(Parameters(ListParams {
            filter: Some("severity>7".into()),
            ..Default::default()
        }))
        .await
        .unwrap(),
    );
    assert_eq!(vulns["vulnerabilities"][0]["name"], "OpenSSH");

    let lp = ListParams::default;
    let sid = |id: &str| SecInfoIdParams { id: id.into() };
    assert_eq!(
        assert_ok(&mcp.list_cves(Parameters(lp())).await.unwrap())["cves"][0]["id"],
        "CVE-2024-3094"
    );
    assert_eq!(
        assert_ok(&mcp.get_cve(Parameters(sid("CVE-2024-3094"))).await.unwrap())["id"],
        "CVE-2024-3094"
    );
    assert_eq!(
        assert_ok(&mcp.list_cpes(Parameters(lp())).await.unwrap())["cpes"][0]["id"],
        "cpe:/a:openbsd:openssh"
    );
    assert_eq!(
        assert_ok(
            &mcp.get_cpe(Parameters(sid("cpe:/a:openbsd:openssh")))
                .await
                .unwrap()
        )["id"],
        "cpe:/a:openbsd:openssh"
    );
    assert_eq!(
        assert_ok(
            &mcp.list_cert_bund_advisories(Parameters(lp()))
                .await
                .unwrap()
        )["certBundAdvisories"][0]["id"],
        "CB-K24/0001"
    );
    assert_eq!(
        assert_ok(
            &mcp.get_cert_bund_advisory(Parameters(sid("CB-K24/0001")))
                .await
                .unwrap()
        )["id"],
        "CB-K24/0001"
    );
    assert_eq!(
        assert_ok(
            &mcp.list_dfn_cert_advisories(Parameters(lp()))
                .await
                .unwrap()
        )["dfnCertAdvisories"][0]["id"],
        "DFN-CERT-2024-0001"
    );
    assert_eq!(
        assert_ok(
            &mcp.get_dfn_cert_advisory(Parameters(sid("DFN-CERT-2024-0001")))
                .await
                .unwrap()
        )["id"],
        "DFN-CERT-2024-0001"
    );
}

// ───── NVT typed query, feeds type filter

#[tokio::test]
async fn nvt_search_uses_typed_selectors_not_filter() {
    let (server, mcp) = server().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/nvts"))
        .and(query_param("configId", "sc-1"))
        .and(query_param("family", "Web Servers"))
        .and(query_param("includePreferences", "true"))
        .and(query_param("sortOrder", "descending"))
        .and(query_param("sortField", "cvss_base"))
        .and(query_param("perPage", "3"))
        .and(query_param_is_missing("filter"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(list_body(serde_json::json!({
                "oid": "1.2", "name": "Apache", "family": "Web Servers", "severity": 5.0
            }))),
        )
        .expect(1)
        .mount(&server)
        .await;
    let json = assert_ok(
        &mcp.search_nvts(Parameters(SearchNvtsParams {
            page: PageParams {
                page: None,
                per_page: Some(3),
            },
            config_id: Some("sc-1".into()),
            preferences_config_id: None,
            family: Some("Web Servers".into()),
            include_preferences: Some(true),
            include_preference_count: None,
            include_timeout: None,
            sort_order: Some("descending".into()),
            sort_field: Some("cvss_base".into()),
        }))
        .await
        .unwrap(),
    );
    assert_eq!(json["nvts"][0]["oid"], "1.2");
}

#[tokio::test]
async fn feeds_pass_type_and_keep_access_flags() {
    let (server, mcp) = server().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/feeds"))
        .and(query_param("type", "NVT"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{"type": "NVT", "name": "Community", "version": "2026", "currentlySyncing": false}],
            "feedOwnerConfigured": true,
            "feedRolesConfigured": true,
            "feedResourcesAccess": false
        })))
        .expect(1)
        .mount(&server)
        .await;
    let json = assert_ok(
        &mcp.list_feeds(Parameters(ListFeedsParams {
            feed_type: Some("NVT".into()),
        }))
        .await
        .unwrap(),
    );
    assert_eq!(json["feeds"][0]["type"], "NVT");
    assert_eq!(json["feedOwnerConfigured"], true);
    assert_eq!(json["feedResourcesAccess"], false);
}

// ───── filters, tags writes; port list name; user settings filter

#[tokio::test]
async fn filter_and_tag_writes() {
    let (server, mcp) = server().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/filters"))
        .and(body_json(serde_json::json!({
            "name": "crit", "term": "severity>9", "type": "result"
        })))
        .respond_with(created())
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/api/v1/filters/f-1"))
        .and(body_json(serde_json::json!({"term": "severity>8"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": "f-1"})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/filters/x-1"))
        .and(query_param("ultimate", "true"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/tags"))
        .and(body_json(serde_json::json!({
            "name": "env", "value": "prod", "resourceType": "task", "resourceId": "t-1",
            "active": true
        })))
        .respond_with(created())
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/api/v1/tags/tag-1"))
        .and(body_json(
            serde_json::json!({"value": "staging", "active": false}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": "tag-1"})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/tags/x-1"))
        .and(query_param("ultimate", "true"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    assert_eq!(
        assert_ok(
            &mcp.create_filter(Parameters(CreateFilterParams {
                name: "crit".into(),
                comment: None,
                term: Some("severity>9".into()),
                filter_type: Some("result".into()),
            }))
            .await
            .unwrap()
        )["id"],
        "new-id"
    );
    assert_eq!(
        assert_ok(
            &mcp.update_filter(Parameters(UpdateFilterParams {
                id: "f-1".into(),
                comment: None,
                term: Some("severity>8".into()),
                filter_type: None,
            }))
            .await
            .unwrap()
        )["id"],
        "f-1"
    );
    assert_eq!(
        assert_ok(&mcp.delete_filter(Parameters(ultimate())).await.unwrap())["deleted"],
        true
    );
    assert_eq!(
        assert_ok(
            &mcp.create_tag(Parameters(CreateTagParams {
                name: "env".into(),
                comment: None,
                value: Some("prod".into()),
                resource_type: Some("task".into()),
                resource_id: Some("t-1".into()),
                active: Some(true),
            }))
            .await
            .unwrap()
        )["id"],
        "new-id"
    );
    assert_eq!(
        assert_ok(
            &mcp.update_tag(Parameters(UpdateTagParams {
                id: "tag-1".into(),
                comment: None,
                value: Some("staging".into()),
                resource_type: None,
                resource_id: None,
                active: Some(false),
            }))
            .await
            .unwrap()
        )["id"],
        "tag-1"
    );
    assert_eq!(
        assert_ok(&mcp.delete_tag(Parameters(ultimate())).await.unwrap())["deleted"],
        true
    );
}

#[tokio::test]
async fn port_list_update_sends_name_and_user_settings_filter_only() {
    let (server, mcp) = server().await;
    Mock::given(method("PUT"))
        .and(path("/api/v1/port-lists/pl-1"))
        .and(body_json(
            serde_json::json!({"name": "web ports", "portRange": "T:80,443"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": "pl-1"})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/user-settings"))
        .and(query_param("filter", "name~Rows"))
        .and(query_param_is_missing("page"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{"id": "s-1", "name": "Rows Per Page", "value": "50"}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    assert_eq!(
        assert_ok(
            &mcp.update_port_list(Parameters(UpdatePortListParams {
                id: "pl-1".into(),
                name: Some("web ports".into()),
                comment: None,
                port_range: Some("T:80,443".into()),
            }))
            .await
            .unwrap()
        )["id"],
        "pl-1"
    );
    let identity = GvmMcpServer::new(support::config_with_args(
        &server,
        &["--toolsets", "identity"],
    ))
    .unwrap();
    let settings = assert_ok(
        &identity
            .list_user_settings(Parameters(FilterParams {
                filter: Some("name~Rows".into()),
                filter_id: None,
            }))
            .await
            .unwrap(),
    );
    assert_eq!(settings["settings"][0]["value"], "50");
}

// ───── specialized targets

#[tokio::test]
async fn oci_and_web_application_targets_crud() {
    let (server, mcp) = server().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/oci-image-targets"))
        .and(query_param("trash", "true"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(list_body(serde_json::json!({
                "id": "oci-1", "name": "images", "imageReferences": ["reg/app:1"], "tasks": [],
                "inUse": false, "writable": true
            }))),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/oci-image-targets/oci-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": "oci-1"})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/oci-image-targets"))
        .and(body_json(serde_json::json!({
            "name": "images", "imageReferences": ["reg/app:1"], "credentialId": "c-1"
        })))
        .respond_with(created())
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/api/v1/oci-image-targets/oci-1"))
        .and(body_json(
            serde_json::json!({"imageReferences": ["reg/app:2"]}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": "oci-1"})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/oci-image-targets/x-1"))
        .and(query_param("ultimate", "true"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/web-application-targets"))
        .and(query_param_is_missing("trash"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(list_body(serde_json::json!({
                "id": "wat-1", "name": "shop", "urls": ["https://shop.example"], "excludeUrls": [],
                "tasks": [], "inUse": false, "writable": true
            }))),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/web-application-targets/wat-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": "wat-1"})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/web-application-targets"))
        .and(body_json(serde_json::json!({
            "name": "shop", "urls": ["https://shop.example"],
            "excludeUrls": ["https://shop.example/logout"]
        })))
        .respond_with(created())
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/api/v1/web-application-targets/wat-1"))
        .and(body_json(serde_json::json!({"name": "shop v2"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": "wat-1"})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/web-application-targets/x-1"))
        .and(query_param("ultimate", "true"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let oci = assert_ok(
        &mcp.list_oci_image_targets(Parameters(TrashListParams {
            list: ListParams::default(),
            trash: Some(true),
        }))
        .await
        .unwrap(),
    );
    assert_eq!(oci["ociImageTargets"][0]["imageReferences"][0], "reg/app:1");
    assert!(oci["ociImageTargets"][0].get("writable").is_none());
    assert_eq!(
        assert_ok(
            &mcp.get_oci_image_target(Parameters(GetByIdParams { id: "oci-1".into() }))
                .await
                .unwrap()
        )["id"],
        "oci-1"
    );
    assert_eq!(
        assert_ok(
            &mcp.create_oci_image_target(Parameters(CreateOciImageTargetParams {
                name: "images".into(),
                image_references: vec!["reg/app:1".into()],
                comment: None,
                credential_id: Some("c-1".into()),
            }))
            .await
            .unwrap()
        )["id"],
        "new-id"
    );
    assert_eq!(
        assert_ok(
            &mcp.update_oci_image_target(Parameters(UpdateOciImageTargetParams {
                id: "oci-1".into(),
                name: None,
                comment: None,
                image_references: Some(vec!["reg/app:2".into()]),
                credential_id: None,
            }))
            .await
            .unwrap()
        )["id"],
        "oci-1"
    );
    assert_eq!(
        assert_ok(
            &mcp.delete_oci_image_target(Parameters(ultimate()))
                .await
                .unwrap()
        )["deleted"],
        true
    );

    let web = assert_ok(
        &mcp.list_web_application_targets(Parameters(TrashListParams::default()))
            .await
            .unwrap(),
    );
    assert_eq!(
        web["webApplicationTargets"][0]["urls"][0],
        "https://shop.example"
    );
    assert_eq!(
        assert_ok(
            &mcp.get_web_application_target(Parameters(GetByIdParams { id: "wat-1".into() }))
                .await
                .unwrap()
        )["id"],
        "wat-1"
    );
    assert_eq!(
        assert_ok(
            &mcp.create_web_application_target(Parameters(CreateWebApplicationTargetParams {
                name: "shop".into(),
                urls: vec!["https://shop.example".into()],
                comment: None,
                exclude_urls: Some(vec!["https://shop.example/logout".into()]),
                credential_id: None,
            }))
            .await
            .unwrap()
        )["id"],
        "new-id"
    );
    assert_eq!(
        assert_ok(
            &mcp.update_web_application_target(Parameters(UpdateWebApplicationTargetParams {
                id: "wat-1".into(),
                name: Some("shop v2".into()),
                comment: None,
                urls: None,
                exclude_urls: None,
                credential_id: None,
            }))
            .await
            .unwrap()
        )["id"],
        "wat-1"
    );
    assert_eq!(
        assert_ok(
            &mcp.delete_web_application_target(Parameters(ultimate()))
                .await
                .unwrap()
        )["deleted"],
        true
    );
}

// ───── agents

#[tokio::test]
async fn agent_routes_match_spec() {
    let (server, mcp) = server().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/agents"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(list_body(serde_json::json!({
                "id": "ag-1", "name": "sensor-1", "status": "active", "version": "1.0",
                "authorized": true, "config": {"heartbeat": {}}, "writable": true
            }))),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/agents/ag-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": "ag-1"})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/api/v1/agents/ag-1"))
        .and(body_json(serde_json::json!({
            "authorized": true, "config": {"heartbeat": {"intervalInSeconds": 30}}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": "ag-1"})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/agents/x-1"))
        .and(query_param_is_missing("ultimate"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/agents/sync"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/agents/ag-1/support-bundle"))
        .and(query_param("days", "7"))
        .and(query_param("encryption", "true"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(b"PK\x03\x04bundle".to_vec())
                .insert_header("content-type", "application/zip")
                .insert_header("content-disposition", "attachment; filename=\"bundle.zip\""),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/api/v1/agent-control-scan-configs/acsc-1"))
        .and(body_json(serde_json::json!({
            "agentDefaults": {"agentControl": {"retry": {"attempts": 3}}},
            "updateToLatest": true
        })))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/scanners/s-1/agent-installer-instruction"))
        .and(query_param("originUrl", "https://gvm.example"))
        .and(query_param("language", "de"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "language": "de", "instruction": "curl ..."
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/agent-groups"))
        .and(query_param("trash", "false"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(list_body(serde_json::json!({
                "id": "grp-1", "name": "edge", "schedulerCronTime": "0 * * * *", "agents": [],
                "inUse": false, "writable": true
            }))),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/agent-groups/grp-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": "grp-1"})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/agent-groups"))
        .and(body_json(serde_json::json!({
            "name": "edge", "schedulerCronTime": "0 * * * *", "agentIds": ["ag-1"]
        })))
        .respond_with(created())
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/api/v1/agent-groups/grp-1"))
        .and(body_json(serde_json::json!({
            "schedulerCronTime": "0 2 * * *", "comment": "nightly"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": "grp-1"})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/agent-groups/x-1"))
        .and(query_param("ultimate", "true"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let agents = assert_ok(
        &mcp.list_agents(Parameters(ListParams::default()))
            .await
            .unwrap(),
    );
    assert_eq!(agents["agents"][0]["authorized"], true);
    assert!(agents["agents"][0].get("config").is_none());
    assert_eq!(
        assert_ok(
            &mcp.get_agent(Parameters(GetByIdParams { id: "ag-1".into() }))
                .await
                .unwrap()
        )["id"],
        "ag-1"
    );
    assert_eq!(
        assert_ok(
            &mcp.update_agent(Parameters(UpdateAgentParams {
                id: "ag-1".into(),
                authorized: Some(true),
                update_to_latest: None,
                comment: None,
                config: Some(serde_json::json!({"heartbeat": {"intervalInSeconds": 30}})),
            }))
            .await
            .unwrap()
        )["id"],
        "ag-1"
    );
    assert_eq!(
        assert_ok(&mcp.delete_agent(Parameters(by_id())).await.unwrap())["deleted"],
        true
    );
    assert_eq!(
        assert_ok(&mcp.sync_agents().await.unwrap())["syncStarted"],
        true
    );
    let bundle = assert_ok(
        &mcp.get_agent_support_bundle(Parameters(AgentSupportBundleParams {
            id: "ag-1".into(),
            days: Some(7),
            encryption: Some(true),
        }))
        .await
        .unwrap(),
    );
    assert_eq!(bundle["contentType"], "application/zip");
    assert_eq!(bundle["filename"], "bundle.zip");
    assert_eq!(bundle["size"], 10);
    assert_eq!(
        assert_ok(
            &mcp.update_agent_control_scan_config(Parameters(UpdateAgentControlScanConfigParams {
                id: "acsc-1".into(),
                agent_defaults: Some(
                    serde_json::json!({"agentControl": {"retry": {"attempts": 3}}})
                ),
                update_to_latest: Some(true),
            }))
            .await
            .unwrap()
        )["updated"],
        true
    );
    assert_eq!(
        assert_ok(
            &mcp.get_agent_installer_instruction(Parameters(AgentInstallerInstructionParams {
                scanner_id: "s-1".into(),
                origin_url: "https://gvm.example".into(),
                language: Some("de".into()),
            }))
            .await
            .unwrap()
        )["language"],
        "de"
    );
    let groups = assert_ok(
        &mcp.list_agent_groups(Parameters(TrashListParams {
            list: ListParams::default(),
            trash: Some(false),
        }))
        .await
        .unwrap(),
    );
    assert_eq!(groups["agentGroups"][0]["schedulerCronTime"], "0 * * * *");
    assert_eq!(
        assert_ok(
            &mcp.get_agent_group(Parameters(GetByIdParams { id: "grp-1".into() }))
                .await
                .unwrap()
        )["id"],
        "grp-1"
    );
    assert_eq!(
        assert_ok(
            &mcp.create_agent_group(Parameters(CreateAgentGroupParams {
                name: "edge".into(),
                scheduler_cron_time: "0 * * * *".into(),
                comment: None,
                agent_ids: Some(vec!["ag-1".into()]),
            }))
            .await
            .unwrap()
        )["id"],
        "new-id"
    );
    assert_eq!(
        assert_ok(
            &mcp.update_agent_group(Parameters(UpdateAgentGroupParams {
                id: "grp-1".into(),
                scheduler_cron_time: "0 2 * * *".into(),
                name: None,
                comment: Some("nightly".into()),
                agent_ids: None,
            }))
            .await
            .unwrap()
        )["id"],
        "grp-1"
    );
    assert_eq!(
        assert_ok(
            &mcp.delete_agent_group(Parameters(ultimate()))
                .await
                .unwrap()
        )["deleted"],
        true
    );
}

// ───── test_connection includes readiness

#[tokio::test]
async fn test_connection_reports_readiness() {
    let (server, mcp) = server().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"status": "ok"})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/ready"))
        .respond_with(ResponseTemplate::new(503).set_body_json(serde_json::json!({
            "status": "notReady", "reason": "gvmd socket unavailable"
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/version"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "apiVersion": "0.5.0", "gmpVersion": "22.8"
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/targets"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [], "pagination": {"page": 1, "perPage": 1, "total": 0, "totalPages": 0}
        })))
        .mount(&server)
        .await;
    let json = assert_ok(&mcp.test_connection().await.unwrap());
    assert_eq!(json["gatewayReadiness"], "notReady");
    assert_eq!(json["gatewayReadinessReason"], "gvmd socket unavailable");
    assert_eq!(json["gmpVersion"], "22.8");
}
