//! Scan-configs toolset: CRUD, the selected-NVT / preference / family
//! selection sub-resources, and the generic usage-typed config catalog.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData as McpError, schemars, tool, tool_router};
use serde::{Deserialize, Serialize};

use crate::mcp::server::GvmMcpServer;

use super::common::{
    Body, DeleteParams, GetByIdParams, ListParams, PageParams, clone_resource, create_resource,
    delete_resource, get_passthrough, get_passthrough_query, list_summarized,
    list_summarized_query, update_resource, update_resource_empty,
};

const ROW_KEYS: &[&str] = &[
    "id",
    "name",
    "comment",
    "familyCount",
    "nvtCount",
    "type",
    "usageType",
    "inUse",
];

const CONFIG_ROW_KEYS: &[&str] = &["id", "name", "comment", "type", "usageType", "inUse"];

const NVT_ROW_KEYS: &[&str] = &[
    "oid",
    "name",
    "family",
    "severity",
    "cvssBase",
    "solutionType",
];

#[tool_router(router = scan_configs_router, vis = "pub(crate)")]
impl GvmMcpServer {
    /// List scan configurations (e.g. "Full and fast"). Returns summarized
    /// rows plus pagination; use openvas_get_scan_config for full details.
    #[tool(
        name = "openvas_list_scan_configs",
        annotations(title = "List scan configs", read_only_hint = true)
    )]
    pub async fn list_scan_configs(
        &self,
        Parameters(params): Parameters<ListParams>,
    ) -> Result<CallToolResult, McpError> {
        list_summarized(
            self.gateway(),
            "scan-configs",
            "scanConfigs",
            ROW_KEYS,
            &params,
            "listing scan configs",
        )
        .await
    }

    /// Get one scan configuration by UUID.
    #[tool(
        name = "openvas_get_scan_config",
        annotations(title = "Get scan config", read_only_hint = true)
    )]
    pub async fn get_scan_config(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        get_passthrough(
            self.gateway(),
            &["scan-configs", &params.id],
            "fetching the scan config",
        )
        .await
    }

    /// Create a scan configuration as a copy of an existing one (gvmd
    /// always copies; pick a base such as "Full and fast" or "Base"). Not
    /// idempotent: repeating the call creates duplicates.
    #[tool(
        name = "openvas_create_scan_config",
        annotations(
            title = "Create scan config",
            read_only_hint = false,
            destructive_hint = false
        )
    )]
    pub async fn create_scan_config(
        &self,
        Parameters(params): Parameters<CreateScanConfigParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set("name", params.name)
            .set("baseScanConfigId", params.base_scan_config_id)
            .set_opt("comment", params.comment);
        create_resource(
            self.gateway(),
            "scan-configs",
            body,
            "creating the scan config",
        )
        .await
    }

    /// Rename or re-comment a scan configuration. Idempotent (PUT); fails
    /// with 409 if the config is predefined or in use.
    #[tool(
        name = "openvas_update_scan_config",
        annotations(
            title = "Update scan config",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn update_scan_config(
        &self,
        Parameters(params): Parameters<UpdateScanConfigParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set_opt("name", params.name)
            .set_opt("comment", params.comment);
        update_resource(
            self.gateway(),
            &["scan-configs", &params.id],
            body,
            "updating the scan config",
        )
        .await
    }

    /// Delete a scan configuration (to the trashcan by default; `ultimate`
    /// deletes permanently). Fails with 409 if a task still uses it.
    #[tool(
        name = "openvas_delete_scan_config",
        annotations(
            title = "Delete scan config",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn delete_scan_config(
        &self,
        Parameters(params): Parameters<DeleteParams>,
    ) -> Result<CallToolResult, McpError> {
        delete_resource(
            self.gateway(),
            "scan-configs",
            &params,
            "deleting the scan config",
        )
        .await
    }

    /// List the NVTs selected by a scan configuration, optionally restricted
    /// to one family. Pagination only (no filter expressions).
    #[tool(
        name = "openvas_list_scan_config_nvts",
        annotations(title = "List scan config NVTs", read_only_hint = true)
    )]
    pub async fn list_scan_config_nvts(
        &self,
        Parameters(params): Parameters<ScanConfigNvtsParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut query = params.page.to_query();
        if let Some(family) = params.family {
            query.push(("family", family));
        }
        list_summarized_query(
            self.gateway(),
            &["scan-configs", &params.id, "nvts"],
            &query,
            "nvts",
            NVT_ROW_KEYS,
            "listing scan config NVTs",
        )
        .await
    }

    /// Get one NVT as selected by a scan configuration (404 if the config
    /// does not select it).
    #[tool(
        name = "openvas_get_scan_config_nvt",
        annotations(title = "Get scan config NVT", read_only_hint = true)
    )]
    pub async fn get_scan_config_nvt(
        &self,
        Parameters(params): Parameters<ScanConfigNvtParams>,
    ) -> Result<CallToolResult, McpError> {
        get_passthrough(
            self.gateway(),
            &["scan-configs", &params.id, "nvts", &params.oid],
            "fetching the scan config NVT",
        )
        .await
    }

    /// List the configured preferences of a scan configuration (scanner
    /// preferences, or one NVT's preferences when `nvt_oid` is given).
    #[tool(
        name = "openvas_list_scan_config_preferences",
        annotations(title = "List scan config preferences", read_only_hint = true)
    )]
    pub async fn list_scan_config_preferences(
        &self,
        Parameters(params): Parameters<ScanConfigPreferencesParams>,
    ) -> Result<CallToolResult, McpError> {
        let query = nvt_oid_query(params.nvt_oid);
        get_passthrough_query(
            self.gateway(),
            &["scan-configs", &params.id, "preferences"],
            &query,
            "listing scan config preferences",
        )
        .await
    }

    /// Get one configured preference of a scan configuration by name
    /// (scoped to an NVT with `nvt_oid`).
    #[tool(
        name = "openvas_get_scan_config_preference",
        annotations(title = "Get scan config preference", read_only_hint = true)
    )]
    pub async fn get_scan_config_preference(
        &self,
        Parameters(params): Parameters<ScanConfigPreferenceParams>,
    ) -> Result<CallToolResult, McpError> {
        let query = nvt_oid_query(params.nvt_oid);
        get_passthrough_query(
            self.gateway(),
            &["scan-configs", &params.id, "preferences", &params.name],
            &query,
            "fetching the scan config preference",
        )
        .await
    }

    /// Set (or, with `value` omitted, reset to default) one preference of a
    /// scan configuration. Scope to an NVT with `nvt_oid` for NVT
    /// preferences. Idempotent (PUT).
    #[tool(
        name = "openvas_set_scan_config_preference",
        annotations(
            title = "Set scan config preference",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn set_scan_config_preference(
        &self,
        Parameters(params): Parameters<SetScanConfigPreferenceParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set_opt("nvtOid", params.nvt_oid)
            .set_opt("value", params.value);
        update_resource_empty(
            self.gateway(),
            &["scan-configs", &params.id, "preferences", &params.name],
            body,
            "setting the scan config preference",
        )
        .await
    }

    /// Replace the family selection of a scan configuration: which NVT
    /// families are enabled, whether each grows with the feed, and whether
    /// all of its NVTs are selected. Idempotent (PUT).
    #[tool(
        name = "openvas_set_scan_config_family_selection",
        annotations(
            title = "Set scan config family selection",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn set_scan_config_family_selection(
        &self,
        Parameters(params): Parameters<SetFamilySelectionParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set_opt_json("families", Some(params.families))
            .set_opt("autoAddNewFamilies", params.auto_add_new_families);
        update_resource_empty(
            self.gateway(),
            &["scan-configs", &params.id, "family-selection"],
            body,
            "setting the scan config family selection",
        )
        .await
    }

    /// Replace the NVT selection of one family in a scan configuration with
    /// exactly the given OIDs. Idempotent (PUT).
    #[tool(
        name = "openvas_set_scan_config_nvt_selection",
        annotations(
            title = "Set scan config NVT selection",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn set_scan_config_nvt_selection(
        &self,
        Parameters(params): Parameters<SetNvtSelectionParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new().set("nvtOids", params.nvt_oids);
        update_resource_empty(
            self.gateway(),
            &[
                "scan-configs",
                &params.id,
                "families",
                &params.family,
                "nvts",
            ],
            body,
            "setting the scan config NVT selection",
        )
        .await
    }

    // ───── Generic configs (scan + audit + policy usage types) ───────────

    /// List configs across usage types (`scan`, `audit`, `policy`) in one
    /// catalog; filter with `usage_type`.
    #[tool(
        name = "openvas_list_configs",
        annotations(title = "List generic configs", read_only_hint = true)
    )]
    pub async fn list_configs(
        &self,
        Parameters(params): Parameters<ListConfigsParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut query = params.list.to_query();
        if let Some(usage_type) = params.usage_type {
            query.push(("usageType", usage_type));
        }
        list_summarized_query(
            self.gateway(),
            &["configs"],
            &query,
            "configs",
            CONFIG_ROW_KEYS,
            "listing configs",
        )
        .await
    }

    /// Get one generic config by UUID (any usage type).
    #[tool(
        name = "openvas_get_config",
        annotations(title = "Get generic config", read_only_hint = true)
    )]
    pub async fn get_config(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        get_passthrough(
            self.gateway(),
            &["configs", &params.id],
            "fetching the config",
        )
        .await
    }

    /// Delete a generic config of any usage type (to the trashcan by
    /// default; `ultimate` deletes permanently).
    #[tool(
        name = "openvas_delete_config",
        annotations(
            title = "Delete generic config",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn delete_config(
        &self,
        Parameters(params): Parameters<DeleteParams>,
    ) -> Result<CallToolResult, McpError> {
        delete_resource(self.gateway(), "configs", &params, "deleting the config").await
    }

    /// Clone a generic config of any usage type. Returns the UUID of the copy.
    #[tool(
        name = "openvas_clone_config",
        annotations(
            title = "Clone generic config",
            read_only_hint = false,
            destructive_hint = false
        )
    )]
    pub async fn clone_config(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        clone_resource(self.gateway(), "configs", &params.id, "cloning the config").await
    }
}

fn nvt_oid_query(nvt_oid: Option<String>) -> Vec<(&'static str, String)> {
    match nvt_oid {
        Some(oid) => vec![("nvtOid", oid)],
        None => Vec::new(),
    }
}

/// Arguments for `openvas_create_scan_config`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateScanConfigParams {
    /// Scan config name
    pub name: String,
    /// UUID of the active scan config to copy from (required by gvmd)
    pub base_scan_config_id: String,
    /// Optional comment
    pub comment: Option<String>,
}

/// Arguments for `openvas_update_scan_config`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateScanConfigParams {
    /// UUID of the scan config to update
    pub id: String,
    pub name: Option<String>,
    pub comment: Option<String>,
}

/// Arguments for `openvas_list_scan_config_nvts`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ScanConfigNvtsParams {
    /// Scan config UUID
    pub id: String,
    /// Restrict to one NVT family
    pub family: Option<String>,
    #[serde(flatten)]
    pub page: PageParams,
}

/// Arguments for `openvas_get_scan_config_nvt`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ScanConfigNvtParams {
    /// Scan config UUID
    pub id: String,
    /// NVT OID
    pub oid: String,
}

/// Arguments for `openvas_list_scan_config_preferences`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ScanConfigPreferencesParams {
    /// Scan config UUID
    pub id: String,
    /// Restrict to one NVT's preferences
    pub nvt_oid: Option<String>,
}

/// Arguments for `openvas_get_scan_config_preference`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ScanConfigPreferenceParams {
    /// Scan config UUID
    pub id: String,
    /// Preference name
    pub name: String,
    /// NVT OID when the preference belongs to an NVT
    pub nvt_oid: Option<String>,
}

/// Arguments for `openvas_set_scan_config_preference`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SetScanConfigPreferenceParams {
    /// Scan config UUID
    pub id: String,
    /// Preference name
    pub name: String,
    /// NVT OID when the preference belongs to an NVT
    pub nvt_oid: Option<String>,
    /// New value; omit to reset the preference to its default
    pub value: Option<String>,
}

/// One family entry for `openvas_set_scan_config_family_selection`.
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FamilySelection {
    /// Family name, e.g. "Web Servers"
    pub name: String,
    /// Automatically include NVTs added to this family by feed updates
    pub growing: bool,
    /// Select every NVT of the family
    pub all: bool,
}

/// Arguments for `openvas_set_scan_config_family_selection`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SetFamilySelectionParams {
    /// Scan config UUID
    pub id: String,
    /// Complete replacement family selection
    pub families: Vec<FamilySelection>,
    /// Automatically add families that appear in future feed updates
    pub auto_add_new_families: Option<bool>,
}

/// Arguments for `openvas_set_scan_config_nvt_selection`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SetNvtSelectionParams {
    /// Scan config UUID
    pub id: String,
    /// Family whose selection is replaced
    pub family: String,
    /// Complete replacement list of NVT OIDs for the family
    pub nvt_oids: Vec<String>,
}

/// Arguments for `openvas_list_configs`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListConfigsParams {
    #[serde(flatten)]
    pub list: ListParams,
    /// Restrict to one usage type: scan, audit or policy
    pub usage_type: Option<String>,
}
