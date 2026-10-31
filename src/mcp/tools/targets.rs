//! Targets toolset: classic scan targets plus the OCI image and web
//! application target families, with CRUD and clone for each.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData as McpError, schemars, tool, tool_router};
use serde::{Deserialize, Serialize};

use crate::gateway::models::{Pagination, Target, TargetList};
use crate::mcp::error::gateway_tool_error;
use crate::mcp::server::GvmMcpServer;

use super::common::{
    Body, DeleteParams, GetByIdParams, ListParams, TrashListParams, clone_resource,
    create_resource, delete_resource, get_passthrough, json_result, list_summarized_query,
    update_resource,
};

const OCI_ROW_KEYS: &[&str] = &["id", "name", "imageReferences", "credential", "inUse"];
const WEB_APP_ROW_KEYS: &[&str] = &["id", "name", "urls", "excludeUrls", "credential", "inUse"];

/// Summarized list row: compact on purpose (LLM token budget). Full details
/// come from `openvas_get_target`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TargetRow {
    id: String,
    name: String,
    hosts: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    port_list: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    in_use: Option<bool>,
}

#[derive(Debug, Serialize)]
struct TargetListResult {
    targets: Vec<TargetRow>,
    pagination: Pagination,
}

fn summarize(target: Target) -> TargetRow {
    TargetRow {
        id: target.id,
        name: target.name,
        hosts: target.hosts,
        port_list: target.port_list.and_then(|pl| pl.name),
        in_use: target.in_use,
    }
}

#[tool_router(router = targets_router, vis = "pub(crate)")]
impl GvmMcpServer {
    /// List scan targets. Returns summarized rows plus pagination; use
    /// openvas_get_target for full details of one target.
    #[tool(
        name = "openvas_list_targets",
        annotations(title = "List targets", read_only_hint = true)
    )]
    pub async fn list_targets(
        &self,
        Parameters(params): Parameters<ListParams>,
    ) -> Result<CallToolResult, McpError> {
        let list: TargetList = match self
            .gateway()
            .get_json_query(&["targets"], &params.to_query())
            .await
        {
            Ok(list) => list,
            Err(err) => return Ok(gateway_tool_error("listing targets", &err)),
        };

        json_result(&TargetListResult {
            targets: list.data.into_iter().map(summarize).collect(),
            pagination: list.pagination,
        })
    }

    /// Get one scan target by UUID, including credentials, alive test and
    /// port list bindings.
    #[tool(
        name = "openvas_get_target",
        annotations(title = "Get target", read_only_hint = true)
    )]
    pub async fn get_target(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .gateway()
            .get_json::<Target>(&["targets", &params.id])
            .await
        {
            Ok(target) => json_result(&target),
            Err(err) => Ok(gateway_tool_error("fetching the target", &err)),
        }
    }

    /// Create a scan target from host entries. Not idempotent: repeating the
    /// call creates duplicates (the gateway rejects duplicate names with 400).
    #[tool(
        name = "openvas_create_target",
        annotations(
            title = "Create target",
            read_only_hint = false,
            destructive_hint = false
        )
    )]
    pub async fn create_target(
        &self,
        Parameters(params): Parameters<CreateTargetParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set("name", params.name)
            .set("hosts", params.hosts)
            .set_opt("comment", params.comment)
            .set_opt("excludeHosts", params.exclude_hosts)
            .set_opt("aliveTest", params.alive_test)
            .set_opt("portListId", params.port_list_id)
            .set_opt("reverseLookupOnly", params.reverse_lookup_only)
            .set_opt("reverseLookupUnify", params.reverse_lookup_unify)
            .set_opt("sshCredentialId", params.ssh_credential_id)
            .set_opt("smbCredentialId", params.smb_credential_id)
            .set_opt("esxiCredentialId", params.esxi_credential_id)
            .set_opt("snmpCredentialId", params.snmp_credential_id);
        create_resource(self.gateway(), "targets", body, "creating the target").await
    }

    /// Update a scan target. Idempotent (PUT): omitted fields stay unchanged;
    /// fails with 404 if the target does not exist, 409 if it is in use.
    #[tool(
        name = "openvas_update_target",
        annotations(
            title = "Update target",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn update_target(
        &self,
        Parameters(params): Parameters<UpdateTargetParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set_opt("name", params.name)
            .set_opt("comment", params.comment)
            .set_opt("hosts", params.hosts)
            .set_opt("excludeHosts", params.exclude_hosts)
            .set_opt("aliveTest", params.alive_test)
            .set_opt("portListId", params.port_list_id)
            .set_opt("reverseLookupOnly", params.reverse_lookup_only)
            .set_opt("reverseLookupUnify", params.reverse_lookup_unify)
            .set_opt("sshCredentialId", params.ssh_credential_id)
            .set_opt("smbCredentialId", params.smb_credential_id)
            .set_opt("esxiCredentialId", params.esxi_credential_id)
            .set_opt("snmpCredentialId", params.snmp_credential_id);
        update_resource(
            self.gateway(),
            &["targets", &params.id],
            body,
            "updating the target",
        )
        .await
    }

    /// Delete a scan target (to the trashcan by default; `ultimate` deletes
    /// permanently). Fails with 409 if a task still uses the target.
    #[tool(
        name = "openvas_delete_target",
        annotations(
            title = "Delete target",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn delete_target(
        &self,
        Parameters(params): Parameters<DeleteParams>,
    ) -> Result<CallToolResult, McpError> {
        delete_resource(self.gateway(), "targets", &params, "deleting the target").await
    }

    /// Clone a scan target. Returns the UUID of the copy.
    #[tool(
        name = "openvas_clone_target",
        annotations(
            title = "Clone target",
            read_only_hint = false,
            destructive_hint = false
        )
    )]
    pub async fn clone_target(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        clone_resource(self.gateway(), "targets", &params.id, "cloning the target").await
    }

    // ───── OCI image targets ─────────────────────────────────────────────

    /// List OCI (container) image targets. `trash` lists trashed ones.
    #[tool(
        name = "openvas_list_oci_image_targets",
        annotations(title = "List OCI image targets", read_only_hint = true)
    )]
    pub async fn list_oci_image_targets(
        &self,
        Parameters(params): Parameters<TrashListParams>,
    ) -> Result<CallToolResult, McpError> {
        list_summarized_query(
            self.gateway(),
            &["oci-image-targets"],
            &params.to_query(),
            "ociImageTargets",
            OCI_ROW_KEYS,
            "listing OCI image targets",
        )
        .await
    }

    /// Get one OCI image target by UUID.
    #[tool(
        name = "openvas_get_oci_image_target",
        annotations(title = "Get OCI image target", read_only_hint = true)
    )]
    pub async fn get_oci_image_target(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        get_passthrough(
            self.gateway(),
            &["oci-image-targets", &params.id],
            "fetching the OCI image target",
        )
        .await
    }

    /// Create an OCI image target from one or more image references (e.g.
    /// `registry.example/app:1.2`). Not idempotent.
    #[tool(
        name = "openvas_create_oci_image_target",
        annotations(
            title = "Create OCI image target",
            read_only_hint = false,
            destructive_hint = false
        )
    )]
    pub async fn create_oci_image_target(
        &self,
        Parameters(params): Parameters<CreateOciImageTargetParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set("name", params.name)
            .set("imageReferences", params.image_references)
            .set_opt("comment", params.comment)
            .set_opt("credentialId", params.credential_id);
        create_resource(
            self.gateway(),
            "oci-image-targets",
            body,
            "creating the OCI image target",
        )
        .await
    }

    /// Update an OCI image target. Idempotent (PUT): omitted fields stay
    /// unchanged.
    #[tool(
        name = "openvas_update_oci_image_target",
        annotations(
            title = "Update OCI image target",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn update_oci_image_target(
        &self,
        Parameters(params): Parameters<UpdateOciImageTargetParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set_opt("name", params.name)
            .set_opt("comment", params.comment)
            .set_opt("imageReferences", params.image_references)
            .set_opt("credentialId", params.credential_id);
        update_resource(
            self.gateway(),
            &["oci-image-targets", &params.id],
            body,
            "updating the OCI image target",
        )
        .await
    }

    /// Delete an OCI image target (to the trashcan by default; `ultimate`
    /// deletes permanently).
    #[tool(
        name = "openvas_delete_oci_image_target",
        annotations(
            title = "Delete OCI image target",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn delete_oci_image_target(
        &self,
        Parameters(params): Parameters<DeleteParams>,
    ) -> Result<CallToolResult, McpError> {
        delete_resource(
            self.gateway(),
            "oci-image-targets",
            &params,
            "deleting the OCI image target",
        )
        .await
    }

    /// Clone an OCI image target. Returns the UUID of the copy.
    #[tool(
        name = "openvas_clone_oci_image_target",
        annotations(
            title = "Clone OCI image target",
            read_only_hint = false,
            destructive_hint = false
        )
    )]
    pub async fn clone_oci_image_target(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        clone_resource(
            self.gateway(),
            "oci-image-targets",
            &params.id,
            "cloning the OCI image target",
        )
        .await
    }

    // ───── Web application targets ───────────────────────────────────────

    /// List web application targets. `trash` lists trashed ones.
    #[tool(
        name = "openvas_list_web_application_targets",
        annotations(title = "List web application targets", read_only_hint = true)
    )]
    pub async fn list_web_application_targets(
        &self,
        Parameters(params): Parameters<TrashListParams>,
    ) -> Result<CallToolResult, McpError> {
        list_summarized_query(
            self.gateway(),
            &["web-application-targets"],
            &params.to_query(),
            "webApplicationTargets",
            WEB_APP_ROW_KEYS,
            "listing web application targets",
        )
        .await
    }

    /// Get one web application target by UUID.
    #[tool(
        name = "openvas_get_web_application_target",
        annotations(title = "Get web application target", read_only_hint = true)
    )]
    pub async fn get_web_application_target(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        get_passthrough(
            self.gateway(),
            &["web-application-targets", &params.id],
            "fetching the web application target",
        )
        .await
    }

    /// Create a web application target from one or more URLs. Not
    /// idempotent.
    #[tool(
        name = "openvas_create_web_application_target",
        annotations(
            title = "Create web application target",
            read_only_hint = false,
            destructive_hint = false
        )
    )]
    pub async fn create_web_application_target(
        &self,
        Parameters(params): Parameters<CreateWebApplicationTargetParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set("name", params.name)
            .set("urls", params.urls)
            .set_opt("comment", params.comment)
            .set_opt("excludeUrls", params.exclude_urls)
            .set_opt("credentialId", params.credential_id);
        create_resource(
            self.gateway(),
            "web-application-targets",
            body,
            "creating the web application target",
        )
        .await
    }

    /// Update a web application target. Idempotent (PUT): omitted fields
    /// stay unchanged.
    #[tool(
        name = "openvas_update_web_application_target",
        annotations(
            title = "Update web application target",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn update_web_application_target(
        &self,
        Parameters(params): Parameters<UpdateWebApplicationTargetParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set_opt("name", params.name)
            .set_opt("comment", params.comment)
            .set_opt("urls", params.urls)
            .set_opt("excludeUrls", params.exclude_urls)
            .set_opt("credentialId", params.credential_id);
        update_resource(
            self.gateway(),
            &["web-application-targets", &params.id],
            body,
            "updating the web application target",
        )
        .await
    }

    /// Delete a web application target (to the trashcan by default;
    /// `ultimate` deletes permanently).
    #[tool(
        name = "openvas_delete_web_application_target",
        annotations(
            title = "Delete web application target",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn delete_web_application_target(
        &self,
        Parameters(params): Parameters<DeleteParams>,
    ) -> Result<CallToolResult, McpError> {
        delete_resource(
            self.gateway(),
            "web-application-targets",
            &params,
            "deleting the web application target",
        )
        .await
    }

    /// Clone a web application target. Returns the UUID of the copy.
    #[tool(
        name = "openvas_clone_web_application_target",
        annotations(
            title = "Clone web application target",
            read_only_hint = false,
            destructive_hint = false
        )
    )]
    pub async fn clone_web_application_target(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        clone_resource(
            self.gateway(),
            "web-application-targets",
            &params.id,
            "cloning the web application target",
        )
        .await
    }
}

/// Arguments for `openvas_create_target`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateTargetParams {
    /// Target name
    pub name: String,
    /// Host entries: IPs, CIDR ranges, hostnames
    pub hosts: Vec<String>,
    /// Optional comment
    pub comment: Option<String>,
    /// Hosts to exclude from scans
    pub exclude_hosts: Option<Vec<String>>,
    /// Alive test method. One of: "Scan Config Default", "ICMP Ping",
    /// "TCP-ACK Service Ping", "TCP-SYN Service Ping", "ARP Ping",
    /// "ICMP, TCP-ACK Service Ping", "ICMP, ARP Ping",
    /// "TCP-ACK Service, ARP Ping", "ICMP, TCP-ACK Service, ARP Ping",
    /// "Consider Alive"
    pub alive_test: Option<String>,
    /// UUID of the port list to scan (gvmd requires one before a task can
    /// run; see openvas_list_port_lists)
    pub port_list_id: Option<String>,
    /// Only scan hosts that resolve via reverse lookup
    pub reverse_lookup_only: Option<bool>,
    /// Deduplicate hosts that reverse-resolve to the same name
    pub reverse_lookup_unify: Option<bool>,
    /// SSH credential UUID for authenticated scans
    pub ssh_credential_id: Option<String>,
    /// SMB credential UUID for authenticated scans
    pub smb_credential_id: Option<String>,
    /// ESXi credential UUID for authenticated scans
    pub esxi_credential_id: Option<String>,
    /// SNMP credential UUID for authenticated scans
    pub snmp_credential_id: Option<String>,
}

/// Arguments for `openvas_update_target`: all fields optional, omitted
/// fields stay unchanged.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateTargetParams {
    /// UUID of the target to update
    pub id: String,
    pub name: Option<String>,
    pub comment: Option<String>,
    /// Replacement host entries
    pub hosts: Option<Vec<String>>,
    pub exclude_hosts: Option<Vec<String>>,
    /// Alive test method (same values as openvas_create_target)
    pub alive_test: Option<String>,
    pub port_list_id: Option<String>,
    pub reverse_lookup_only: Option<bool>,
    pub reverse_lookup_unify: Option<bool>,
    pub ssh_credential_id: Option<String>,
    pub smb_credential_id: Option<String>,
    pub esxi_credential_id: Option<String>,
    pub snmp_credential_id: Option<String>,
}

/// Arguments for `openvas_create_oci_image_target`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateOciImageTargetParams {
    /// Target name
    pub name: String,
    /// Image references, e.g. "registry.example/app:1.2" (at least one)
    pub image_references: Vec<String>,
    /// Optional comment
    pub comment: Option<String>,
    /// UUID of a registry credential
    pub credential_id: Option<String>,
}

/// Arguments for `openvas_update_oci_image_target`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateOciImageTargetParams {
    /// UUID of the OCI image target to update
    pub id: String,
    pub name: Option<String>,
    pub comment: Option<String>,
    /// Replacement image references (at least one)
    pub image_references: Option<Vec<String>>,
    pub credential_id: Option<String>,
}

/// Arguments for `openvas_create_web_application_target`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateWebApplicationTargetParams {
    /// Target name
    pub name: String,
    /// Start URLs, e.g. "https://shop.example" (at least one)
    pub urls: Vec<String>,
    /// Optional comment
    pub comment: Option<String>,
    /// URLs excluded from crawling
    pub exclude_urls: Option<Vec<String>>,
    /// UUID of a credential for authenticated crawling
    pub credential_id: Option<String>,
}

/// Arguments for `openvas_update_web_application_target`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateWebApplicationTargetParams {
    /// UUID of the web application target to update
    pub id: String,
    pub name: Option<String>,
    pub comment: Option<String>,
    /// Replacement start URLs (at least one)
    pub urls: Option<Vec<String>>,
    pub exclude_urls: Option<Vec<String>>,
    pub credential_id: Option<String>,
}
