//! Tags toolset: tags on GVM resources with CRUD and clone.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData as McpError, schemars, tool, tool_router};
use serde::Deserialize;

use crate::mcp::server::GvmMcpServer;

use super::common::{
    Body, DeleteParams, GetByIdParams, ListParams, clone_resource, create_resource,
    delete_resource, get_passthrough, list_summarized, update_resource,
};

const ROW_KEYS: &[&str] = &[
    "id",
    "name",
    "value",
    "resourceType",
    "resourceCount",
    "active",
];

/// Arguments for `openvas_create_tag`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateTagParams {
    /// Tag name, e.g. "owner" or "env:prod"
    pub name: String,
    /// Optional comment
    pub comment: Option<String>,
    /// Tag value
    pub value: Option<String>,
    /// Type of the resource to attach to, e.g. "task", "target", "host"
    pub resource_type: Option<String>,
    /// UUID of the resource to attach to
    pub resource_id: Option<String>,
    /// Whether the tag is active
    pub active: Option<bool>,
}

/// Arguments for `openvas_update_tag`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateTagParams {
    /// UUID of the tag to update
    pub id: String,
    pub comment: Option<String>,
    pub value: Option<String>,
    pub resource_type: Option<String>,
    pub resource_id: Option<String>,
    pub active: Option<bool>,
}

#[tool_router(router = tags_router, vis = "pub(crate)")]
impl GvmMcpServer {
    /// List tags attached to GVM resources.
    #[tool(
        name = "openvas_list_tags",
        annotations(title = "List tags", read_only_hint = true)
    )]
    pub async fn list_tags(
        &self,
        Parameters(params): Parameters<ListParams>,
    ) -> Result<CallToolResult, McpError> {
        list_summarized(
            self.gateway(),
            "tags",
            "tags",
            ROW_KEYS,
            &params,
            "listing tags",
        )
        .await
    }

    /// Get one tag by UUID, including the resources it is attached to.
    #[tool(
        name = "openvas_get_tag",
        annotations(title = "Get tag", read_only_hint = true)
    )]
    pub async fn get_tag(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        get_passthrough(self.gateway(), &["tags", &params.id], "fetching the tag").await
    }

    /// Create a tag, optionally attached to one resource. Not idempotent.
    #[tool(
        name = "openvas_create_tag",
        annotations(title = "Create tag", read_only_hint = false, destructive_hint = false)
    )]
    pub async fn create_tag(
        &self,
        Parameters(params): Parameters<CreateTagParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set("name", params.name)
            .set_opt("comment", params.comment)
            .set_opt("value", params.value)
            .set_opt("resourceType", params.resource_type)
            .set_opt("resourceId", params.resource_id)
            .set_opt("active", params.active);
        create_resource(self.gateway(), "tags", body, "creating the tag").await
    }

    /// Update a tag's value, attachment or active flag. Idempotent (PUT).
    #[tool(
        name = "openvas_update_tag",
        annotations(title = "Update tag", read_only_hint = false, destructive_hint = true)
    )]
    pub async fn update_tag(
        &self,
        Parameters(params): Parameters<UpdateTagParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set_opt("comment", params.comment)
            .set_opt("value", params.value)
            .set_opt("resourceType", params.resource_type)
            .set_opt("resourceId", params.resource_id)
            .set_opt("active", params.active);
        update_resource(
            self.gateway(),
            &["tags", &params.id],
            body,
            "updating the tag",
        )
        .await
    }

    /// Delete a tag (to the trashcan by default; `ultimate` deletes
    /// permanently).
    #[tool(
        name = "openvas_delete_tag",
        annotations(title = "Delete tag", read_only_hint = false, destructive_hint = true)
    )]
    pub async fn delete_tag(
        &self,
        Parameters(params): Parameters<DeleteParams>,
    ) -> Result<CallToolResult, McpError> {
        delete_resource(self.gateway(), "tags", &params, "deleting the tag").await
    }

    /// Clone a tag. Returns the UUID of the copy.
    #[tool(
        name = "openvas_clone_tag",
        annotations(title = "Clone tag", read_only_hint = false, destructive_hint = false)
    )]
    pub async fn clone_tag(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        clone_resource(self.gateway(), "tags", &params.id, "cloning the tag").await
    }
}
