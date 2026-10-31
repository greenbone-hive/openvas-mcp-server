//! Identity toolset: users, groups, roles, permissions and user settings.
//!
//! Off by default (roadmap: identity is opt-in via `--toolsets identity`):
//! these tools administer accounts and access control, which most scanning
//! workflows never need and which deserve an explicit decision to expose.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData as McpError, schemars, tool, tool_router};
use serde::Deserialize;

use crate::mcp::server::GvmMcpServer;

use super::common::{
    Body, DeleteParams, FilterParams, GetByIdParams, ListParams, create_resource, delete_resource,
    get_passthrough, list_summarized, list_summarized_query, update_resource,
};

const USER_ROW_KEYS: &[&str] = &["id", "name", "roles", "groups", "hostsAllow", "inUse"];
const GROUP_ROW_KEYS: &[&str] = &["id", "name", "comment", "users", "inUse"];
const PERMISSION_ROW_KEYS: &[&str] = &[
    "id",
    "name",
    "subjectType",
    "subject",
    "resourceType",
    "resource",
];
const SETTING_ROW_KEYS: &[&str] = &["id", "name", "value"];

/// Arguments for `openvas_create_user`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateUserParams {
    /// Login name
    pub name: String,
    /// Optional comment
    pub comment: Option<String>,
    /// Initial password (required for `file` authentication)
    pub password: Option<String>,
    /// Host restriction expression as stored by gvmd
    pub hosts: Option<String>,
    /// Role UUIDs to assign
    pub roles: Option<Vec<String>>,
    /// Authentication backend: file, ldap_connect or radius_connect
    pub authentication_type: Option<String>,
}

/// Arguments for `openvas_update_user`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateUserParams {
    /// UUID of the user to update
    pub id: String,
    /// New login name
    pub name: Option<String>,
    pub comment: Option<String>,
    /// New password
    pub password: Option<String>,
    pub hosts: Option<String>,
    /// Replacement role UUIDs (omitting leaves roles unchanged)
    pub roles: Option<Vec<String>>,
    pub authentication_type: Option<String>,
}

/// Arguments for `openvas_create_group` / `openvas_create_role`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateMembershipParams {
    /// Name
    pub name: String,
    /// Optional comment
    pub comment: Option<String>,
    /// Member login names
    pub users: Option<Vec<String>>,
}

/// Arguments for `openvas_update_group` / `openvas_update_role`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateMembershipParams {
    /// UUID of the group/role to update
    pub id: String,
    pub comment: Option<String>,
    /// Replacement member login names
    pub users: Option<Vec<String>>,
}

/// Arguments for `openvas_create_permission`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreatePermissionParams {
    /// Permission name (a GMP command name, e.g. "get_targets")
    pub name: String,
    /// Optional comment
    pub comment: Option<String>,
    /// Who receives the grant: user, group or role
    pub subject_type: Option<String>,
    /// UUID of the subject
    pub subject_id: Option<String>,
    /// Backend resource type of the grant target (e.g. "task")
    pub resource_type: Option<String>,
    /// UUID of the grant target resource
    pub resource_id: Option<String>,
}

/// Arguments for `openvas_update_permission`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdatePermissionParams {
    /// UUID of the permission to update
    pub id: String,
    pub name: Option<String>,
    pub comment: Option<String>,
    pub subject_type: Option<String>,
    pub subject_id: Option<String>,
    pub resource_type: Option<String>,
    pub resource_id: Option<String>,
}

/// Arguments for `openvas_update_user_setting`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateUserSettingParams {
    /// UUID of the setting
    pub id: String,
    /// New value
    pub value: String,
}

#[tool_router(router = identity_router, vis = "pub(crate)")]
impl GvmMcpServer {
    /// List gvmd user accounts with their roles and groups.
    #[tool(
        name = "openvas_list_users",
        annotations(title = "List users", read_only_hint = true)
    )]
    pub async fn list_users(
        &self,
        Parameters(params): Parameters<ListParams>,
    ) -> Result<CallToolResult, McpError> {
        list_summarized(
            self.gateway(),
            "users",
            "users",
            USER_ROW_KEYS,
            &params,
            "listing users",
        )
        .await
    }

    /// Get one user account by UUID.
    #[tool(
        name = "openvas_get_user",
        annotations(title = "Get user", read_only_hint = true)
    )]
    pub async fn get_user(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        get_passthrough(self.gateway(), &["users", &params.id], "fetching the user").await
    }

    /// Create a gvmd user account. Not idempotent.
    #[tool(
        name = "openvas_create_user",
        annotations(
            title = "Create user",
            read_only_hint = false,
            destructive_hint = false
        )
    )]
    pub async fn create_user(
        &self,
        Parameters(params): Parameters<CreateUserParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set("name", params.name)
            .set_opt("comment", params.comment)
            .set_opt("password", params.password)
            .set_opt("hosts", params.hosts)
            .set_opt("roles", params.roles)
            .set_opt("authenticationType", params.authentication_type);
        create_resource(self.gateway(), "users", body, "creating the user").await
    }

    /// Update a user account (password, roles, host restriction).
    /// Idempotent (PUT): omitted fields stay unchanged.
    #[tool(
        name = "openvas_update_user",
        annotations(title = "Update user", read_only_hint = false, destructive_hint = true)
    )]
    pub async fn update_user(
        &self,
        Parameters(params): Parameters<UpdateUserParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set_opt("name", params.name)
            .set_opt("comment", params.comment)
            .set_opt("password", params.password)
            .set_opt("hosts", params.hosts)
            .set_opt("roles", params.roles)
            .set_opt("authenticationType", params.authentication_type);
        update_resource(
            self.gateway(),
            &["users", &params.id],
            body,
            "updating the user",
        )
        .await
    }

    /// Delete a user account (to the trashcan by default; `ultimate`
    /// deletes permanently).
    #[tool(
        name = "openvas_delete_user",
        annotations(title = "Delete user", read_only_hint = false, destructive_hint = true)
    )]
    pub async fn delete_user(
        &self,
        Parameters(params): Parameters<DeleteParams>,
    ) -> Result<CallToolResult, McpError> {
        delete_resource(self.gateway(), "users", &params, "deleting the user").await
    }

    /// List user groups.
    #[tool(
        name = "openvas_list_groups",
        annotations(title = "List groups", read_only_hint = true)
    )]
    pub async fn list_groups(
        &self,
        Parameters(params): Parameters<ListParams>,
    ) -> Result<CallToolResult, McpError> {
        list_summarized(
            self.gateway(),
            "groups",
            "groups",
            GROUP_ROW_KEYS,
            &params,
            "listing groups",
        )
        .await
    }

    /// Get one user group by UUID.
    #[tool(
        name = "openvas_get_group",
        annotations(title = "Get group", read_only_hint = true)
    )]
    pub async fn get_group(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        get_passthrough(
            self.gateway(),
            &["groups", &params.id],
            "fetching the group",
        )
        .await
    }

    /// Create a user group. Not idempotent.
    #[tool(
        name = "openvas_create_group",
        annotations(
            title = "Create group",
            read_only_hint = false,
            destructive_hint = false
        )
    )]
    pub async fn create_group(
        &self,
        Parameters(params): Parameters<CreateMembershipParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set("name", params.name)
            .set_opt("comment", params.comment)
            .set_opt("users", params.users);
        create_resource(self.gateway(), "groups", body, "creating the group").await
    }

    /// Update a user group's members or comment. Idempotent (PUT).
    #[tool(
        name = "openvas_update_group",
        annotations(
            title = "Update group",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn update_group(
        &self,
        Parameters(params): Parameters<UpdateMembershipParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set_opt("comment", params.comment)
            .set_opt("users", params.users);
        update_resource(
            self.gateway(),
            &["groups", &params.id],
            body,
            "updating the group",
        )
        .await
    }

    /// Delete a user group (to the trashcan by default; `ultimate` deletes
    /// permanently).
    #[tool(
        name = "openvas_delete_group",
        annotations(
            title = "Delete group",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn delete_group(
        &self,
        Parameters(params): Parameters<DeleteParams>,
    ) -> Result<CallToolResult, McpError> {
        delete_resource(self.gateway(), "groups", &params, "deleting the group").await
    }

    /// List roles.
    #[tool(
        name = "openvas_list_roles",
        annotations(title = "List roles", read_only_hint = true)
    )]
    pub async fn list_roles(
        &self,
        Parameters(params): Parameters<ListParams>,
    ) -> Result<CallToolResult, McpError> {
        list_summarized(
            self.gateway(),
            "roles",
            "roles",
            GROUP_ROW_KEYS,
            &params,
            "listing roles",
        )
        .await
    }

    /// Get one role by UUID.
    #[tool(
        name = "openvas_get_role",
        annotations(title = "Get role", read_only_hint = true)
    )]
    pub async fn get_role(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        get_passthrough(self.gateway(), &["roles", &params.id], "fetching the role").await
    }

    /// Create a role. Not idempotent.
    #[tool(
        name = "openvas_create_role",
        annotations(
            title = "Create role",
            read_only_hint = false,
            destructive_hint = false
        )
    )]
    pub async fn create_role(
        &self,
        Parameters(params): Parameters<CreateMembershipParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set("name", params.name)
            .set_opt("comment", params.comment)
            .set_opt("users", params.users);
        create_resource(self.gateway(), "roles", body, "creating the role").await
    }

    /// Update a role's members or comment. Idempotent (PUT).
    #[tool(
        name = "openvas_update_role",
        annotations(title = "Update role", read_only_hint = false, destructive_hint = true)
    )]
    pub async fn update_role(
        &self,
        Parameters(params): Parameters<UpdateMembershipParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set_opt("comment", params.comment)
            .set_opt("users", params.users);
        update_resource(
            self.gateway(),
            &["roles", &params.id],
            body,
            "updating the role",
        )
        .await
    }

    /// Delete a role (to the trashcan by default; `ultimate` deletes
    /// permanently).
    #[tool(
        name = "openvas_delete_role",
        annotations(title = "Delete role", read_only_hint = false, destructive_hint = true)
    )]
    pub async fn delete_role(
        &self,
        Parameters(params): Parameters<DeleteParams>,
    ) -> Result<CallToolResult, McpError> {
        delete_resource(self.gateway(), "roles", &params, "deleting the role").await
    }

    /// List permissions (GMP command grants).
    #[tool(
        name = "openvas_list_permissions",
        annotations(title = "List permissions", read_only_hint = true)
    )]
    pub async fn list_permissions(
        &self,
        Parameters(params): Parameters<ListParams>,
    ) -> Result<CallToolResult, McpError> {
        list_summarized(
            self.gateway(),
            "permissions",
            "permissions",
            PERMISSION_ROW_KEYS,
            &params,
            "listing permissions",
        )
        .await
    }

    /// Get one permission by UUID.
    #[tool(
        name = "openvas_get_permission",
        annotations(title = "Get permission", read_only_hint = true)
    )]
    pub async fn get_permission(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        get_passthrough(
            self.gateway(),
            &["permissions", &params.id],
            "fetching the permission",
        )
        .await
    }

    /// Grant a permission (a GMP command, optionally scoped to one
    /// resource) to a user, group or role. Not idempotent.
    #[tool(
        name = "openvas_create_permission",
        annotations(
            title = "Create permission",
            read_only_hint = false,
            destructive_hint = false
        )
    )]
    pub async fn create_permission(
        &self,
        Parameters(params): Parameters<CreatePermissionParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set("name", params.name)
            .set_opt("comment", params.comment)
            .set_opt("subjectType", params.subject_type)
            .set_opt("subjectId", params.subject_id)
            .set_opt("resourceType", params.resource_type)
            .set_opt("resourceId", params.resource_id);
        create_resource(
            self.gateway(),
            "permissions",
            body,
            "creating the permission",
        )
        .await
    }

    /// Update a permission grant. Idempotent (PUT).
    #[tool(
        name = "openvas_update_permission",
        annotations(
            title = "Update permission",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn update_permission(
        &self,
        Parameters(params): Parameters<UpdatePermissionParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set_opt("name", params.name)
            .set_opt("comment", params.comment)
            .set_opt("subjectType", params.subject_type)
            .set_opt("subjectId", params.subject_id)
            .set_opt("resourceType", params.resource_type)
            .set_opt("resourceId", params.resource_id);
        update_resource(
            self.gateway(),
            &["permissions", &params.id],
            body,
            "updating the permission",
        )
        .await
    }

    /// Revoke a permission (to the trashcan by default; `ultimate` deletes
    /// permanently).
    #[tool(
        name = "openvas_delete_permission",
        annotations(
            title = "Delete permission",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn delete_permission(
        &self,
        Parameters(params): Parameters<DeleteParams>,
    ) -> Result<CallToolResult, McpError> {
        delete_resource(
            self.gateway(),
            "permissions",
            &params,
            "deleting the permission",
        )
        .await
    }

    /// List the authenticated user's settings (filterable, not paginated).
    #[tool(
        name = "openvas_list_user_settings",
        annotations(title = "List user settings", read_only_hint = true)
    )]
    pub async fn list_user_settings(
        &self,
        Parameters(params): Parameters<FilterParams>,
    ) -> Result<CallToolResult, McpError> {
        list_summarized_query(
            self.gateway(),
            &["user-settings"],
            &params.to_query(),
            "settings",
            SETTING_ROW_KEYS,
            "listing user settings",
        )
        .await
    }

    /// Get one user setting by UUID.
    #[tool(
        name = "openvas_get_user_setting",
        annotations(title = "Get user setting", read_only_hint = true)
    )]
    pub async fn get_user_setting(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        get_passthrough(
            self.gateway(),
            &["user-settings", &params.id],
            "fetching the user setting",
        )
        .await
    }

    /// Change one user setting's value. Idempotent (PUT).
    #[tool(
        name = "openvas_update_user_setting",
        annotations(
            title = "Update user setting",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn update_user_setting(
        &self,
        Parameters(params): Parameters<UpdateUserSettingParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new().set("value", params.value);
        update_resource(
            self.gateway(),
            &["user-settings", &params.id],
            body,
            "updating the user setting",
        )
        .await
    }
}
