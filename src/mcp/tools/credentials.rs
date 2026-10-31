//! Credentials toolset: reads (secrets never leave the gateway),
//! create/update/delete, and credential-store administration.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData as McpError, schemars, tool, tool_router};
use serde::{Deserialize, Serialize};

use crate::mcp::error::gateway_tool_error;
use crate::mcp::server::GvmMcpServer;

use super::common::{
    Body, DeleteParams, GetByIdParams, ListParams, create_resource, delete_resource,
    get_passthrough, json_result, list_summarized, list_summarized_query, update_resource,
};

const ROW_KEYS: &[&str] = &["id", "name", "type", "login", "inUse"];
const STORE_ROW_KEYS: &[&str] = &["id", "name", "provider", "default", "writable"];

#[tool_router(router = credentials_router, vis = "pub(crate)")]
impl GvmMcpServer {
    /// List credentials usable for authenticated scans (metadata only, no
    /// secrets).
    #[tool(
        name = "openvas_list_credentials",
        annotations(title = "List credentials", read_only_hint = true)
    )]
    pub async fn list_credentials(
        &self,
        Parameters(params): Parameters<ListParams>,
    ) -> Result<CallToolResult, McpError> {
        list_summarized(
            self.gateway(),
            "credentials",
            "credentials",
            ROW_KEYS,
            &params,
            "listing credentials",
        )
        .await
    }

    /// List the backend credential stores that `cs_*` credentials can be
    /// created in. Answers 501 on gvmd builds without credential stores.
    #[tool(
        name = "openvas_list_credential_stores",
        annotations(title = "List credential stores", read_only_hint = true)
    )]
    pub async fn list_credential_stores(&self) -> Result<CallToolResult, McpError> {
        list_summarized_query(
            self.gateway(),
            &["credential-stores"],
            &[],
            "credentialStores",
            STORE_ROW_KEYS,
            "listing credential stores",
        )
        .await
    }

    /// Get one credential store (connection settings; preference values are
    /// write-only and never returned).
    #[tool(
        name = "openvas_get_credential_store",
        annotations(title = "Get credential store", read_only_hint = true)
    )]
    pub async fn get_credential_store(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        get_passthrough(
            self.gateway(),
            &["credential-stores", &params.id],
            "fetching the credential store",
        )
        .await
    }

    /// Update a credential store's connection settings. Idempotent (PUT):
    /// omitted fields stay unchanged; preference values are write-only.
    #[tool(
        name = "openvas_update_credential_store",
        annotations(
            title = "Update credential store",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn update_credential_store(
        &self,
        Parameters(params): Parameters<UpdateCredentialStoreParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set_opt("active", params.active)
            .set_opt("host", params.host)
            .set_opt("path", params.path)
            .set_opt("port", params.port)
            .set_opt("comment", params.comment)
            .set_opt_json("preferences", params.preferences);
        update_resource(
            self.gateway(),
            &["credential-stores", &params.id],
            body,
            "updating the credential store",
        )
        .await
    }

    /// Ask gvmd to verify a credential store's connection.
    #[tool(
        name = "openvas_verify_credential_store",
        annotations(
            title = "Verify credential store",
            read_only_hint = false,
            destructive_hint = false
        )
    )]
    pub async fn verify_credential_store(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .gateway()
            .post_action_empty(&["credential-stores", &params.id, "actions", "verify"])
            .await
        {
            Ok(()) => json_result(&serde_json::json!({ "verified": true, "id": params.id })),
            Err(err) => Ok(gateway_tool_error("verifying the credential store", &err)),
        }
    }

    /// Get one credential by UUID (metadata only, no secrets).
    #[tool(
        name = "openvas_get_credential",
        annotations(title = "Get credential", read_only_hint = true)
    )]
    pub async fn get_credential(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        get_passthrough(
            self.gateway(),
            &["credentials", &params.id],
            "fetching the credential",
        )
        .await
    }

    /// Create a credential for authenticated scans. `type` selects the
    /// kind (up, usk, cc, pw, snmp, snmpv3, or a store-backed cs_* type).
    /// Not idempotent.
    #[tool(
        name = "openvas_create_credential",
        annotations(
            title = "Create credential",
            read_only_hint = false,
            destructive_hint = false
        )
    )]
    pub async fn create_credential(
        &self,
        Parameters(params): Parameters<CreateCredentialParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set("name", params.name)
            .set("type", params.credential_type)
            .set_opt("comment", params.comment)
            .set_opt("login", params.login)
            .set_opt("password", params.password)
            .set_opt("privateKey", params.private_key)
            .set_opt("certificate", params.certificate)
            .set_opt("community", params.community)
            .set_opt("authAlgorithm", params.auth_algorithm)
            .set_opt("privacyAlgorithm", params.privacy_algorithm)
            .set_opt("privacyPassword", params.privacy_password)
            .set_opt("credentialStoreId", params.credential_store_id)
            .set_opt("vaultId", params.vault_id)
            .set_opt("hostIdentifier", params.host_identifier);
        create_resource(
            self.gateway(),
            "credentials",
            body,
            "creating the credential",
        )
        .await
    }

    /// Update a credential (the type cannot change). Idempotent (PUT):
    /// omitted fields stay unchanged.
    #[tool(
        name = "openvas_update_credential",
        annotations(
            title = "Update credential",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn update_credential(
        &self,
        Parameters(params): Parameters<UpdateCredentialParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set_opt("name", params.name)
            .set_opt("comment", params.comment)
            .set_opt("login", params.login)
            .set_opt("password", params.password)
            .set_opt("privateKey", params.private_key)
            .set_opt("certificate", params.certificate)
            .set_opt("community", params.community)
            .set_opt("authAlgorithm", params.auth_algorithm)
            .set_opt("privacyAlgorithm", params.privacy_algorithm)
            .set_opt("privacyPassword", params.privacy_password)
            .set_opt("credentialStoreId", params.credential_store_id)
            .set_opt("vaultId", params.vault_id)
            .set_opt("hostIdentifier", params.host_identifier);
        update_resource(
            self.gateway(),
            &["credentials", &params.id],
            body,
            "updating the credential",
        )
        .await
    }

    /// Delete a credential (to the trashcan by default; `ultimate` deletes
    /// permanently). Fails with 409 if a target still uses it.
    #[tool(
        name = "openvas_delete_credential",
        annotations(
            title = "Delete credential",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn delete_credential(
        &self,
        Parameters(params): Parameters<DeleteParams>,
    ) -> Result<CallToolResult, McpError> {
        delete_resource(
            self.gateway(),
            "credentials",
            &params,
            "deleting the credential",
        )
        .await
    }
}

/// Arguments for `openvas_create_credential`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateCredentialParams {
    /// Credential name
    pub name: String,
    /// Credential kind: up (username+password), usk (username+SSH key),
    /// cc (client certificate), pw (password only), snmp (v1/v2c), snmpv3,
    /// or store-backed cs_up, cs_usk, cs_cc, cs_pw, cs_pgp, cs_smime,
    /// cs_snmp
    #[serde(rename = "type")]
    pub credential_type: String,
    /// Optional comment
    pub comment: Option<String>,
    /// Login/username (up, usk, snmp, snmpv3)
    pub login: Option<String>,
    /// Password (up, pw, snmpv3) or key passphrase (usk)
    pub password: Option<String>,
    /// PEM private key (usk)
    pub private_key: Option<String>,
    /// PEM certificate (cc)
    pub certificate: Option<String>,
    /// SNMP community string (snmp)
    pub community: Option<String>,
    /// SNMPv3 auth algorithm: md5 or sha1
    pub auth_algorithm: Option<String>,
    /// SNMPv3 privacy algorithm: aes or des
    pub privacy_algorithm: Option<String>,
    /// SNMPv3 privacy password
    pub privacy_password: Option<String>,
    /// Credential store identifier (cs_* types; see
    /// openvas_list_credential_stores)
    pub credential_store_id: Option<String>,
    /// Write-only external vault identifier (cs_* types)
    pub vault_id: Option<String>,
    /// Write-only external host identifier (cs_* types)
    pub host_identifier: Option<String>,
}

/// Arguments for `openvas_update_credential`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateCredentialParams {
    /// UUID of the credential to update
    pub id: String,
    pub name: Option<String>,
    pub comment: Option<String>,
    pub login: Option<String>,
    pub password: Option<String>,
    pub private_key: Option<String>,
    pub certificate: Option<String>,
    pub community: Option<String>,
    /// md5 or sha1
    pub auth_algorithm: Option<String>,
    /// aes or des
    pub privacy_algorithm: Option<String>,
    pub privacy_password: Option<String>,
    pub credential_store_id: Option<String>,
    pub vault_id: Option<String>,
    pub host_identifier: Option<String>,
}

/// One write-only credential-store preference.
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CredentialStorePreference {
    /// Preference name
    pub name: String,
    /// Preference value (write-only, never returned)
    pub value: String,
}

/// Arguments for `openvas_update_credential_store`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateCredentialStoreParams {
    /// Credential store identifier
    pub id: String,
    pub active: Option<bool>,
    /// Store host
    pub host: Option<String>,
    /// Store path
    pub path: Option<String>,
    /// Store port (0-65535)
    pub port: Option<u16>,
    pub comment: Option<String>,
    /// Write-only preferences (e.g. tokens) for the store backend
    pub preferences: Option<Vec<CredentialStorePreference>>,
}
