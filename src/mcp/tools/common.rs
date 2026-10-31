//! Shared tool parameter shapes and response helpers.

use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::{ErrorData as McpError, schemars};
use serde::{Deserialize, Serialize};

use crate::gateway::GatewayClient;
use crate::mcp::error::gateway_tool_error;

/// Standard listing parameters shared by all gateway list endpoints.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct ListParams {
    /// GMP filter expression, e.g. `name~webserver and severity>5`
    pub filter: Option<String>,
    /// UUID of a saved filter to apply
    pub filter_id: Option<String>,
    /// Page number, 1-indexed (default 1)
    pub page: Option<u32>,
    /// Items per page, 1-1000 (default 25)
    pub per_page: Option<u32>,
}

impl ListParams {
    /// Map to the gateway's query parameters, omitting unset values.
    pub fn to_query(&self) -> Vec<(&'static str, String)> {
        let mut query = Vec::new();
        if let Some(filter) = &self.filter {
            query.push(("filter", filter.clone()));
        }
        if let Some(filter_id) = &self.filter_id {
            query.push(("filterId", filter_id.clone()));
        }
        if let Some(page) = self.page {
            query.push(("page", page.to_string()));
        }
        if let Some(per_page) = self.per_page {
            query.push(("perPage", per_page.to_string()));
        }
        query
    }
}

/// Listing parameters for collections that support the gateway's `trash`
/// selector (agent groups, OCI image and web application targets).
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct TrashListParams {
    #[serde(flatten)]
    pub list: ListParams,
    /// List trashed resources instead of active ones
    pub trash: Option<bool>,
}

impl TrashListParams {
    pub fn to_query(&self) -> Vec<(&'static str, String)> {
        let mut query = self.list.to_query();
        if let Some(trash) = self.trash {
            query.push(("trash", trash.to_string()));
        }
        query
    }
}

/// Pagination-only parameters for collections without filter support
/// (NVT families, scan-config NVT selections).
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct PageParams {
    /// Page number, 1-indexed (default 1)
    pub page: Option<u32>,
    /// Items per page, 1-1000 (default 25)
    pub per_page: Option<u32>,
}

impl PageParams {
    pub fn to_query(&self) -> Vec<(&'static str, String)> {
        let mut query = Vec::new();
        if let Some(page) = self.page {
            query.push(("page", page.to_string()));
        }
        if let Some(per_page) = self.per_page {
            query.push(("perPage", per_page.to_string()));
        }
        query
    }
}

/// Filter-only parameters for collections without pagination (user
/// settings).
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct FilterParams {
    /// GMP filter expression
    pub filter: Option<String>,
    /// UUID of a saved filter to apply
    pub filter_id: Option<String>,
}

impl FilterParams {
    pub fn to_query(&self) -> Vec<(&'static str, String)> {
        let mut query = Vec::new();
        if let Some(filter) = &self.filter {
            query.push(("filter", filter.clone()));
        }
        if let Some(filter_id) = &self.filter_id {
            query.push(("filterId", filter_id.clone()));
        }
        query
    }
}

/// Parameters for tools that address one resource by UUID.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetByIdParams {
    /// Resource UUID
    pub id: String,
}

/// Parameters for delete tools on resources with gvmd trashcan semantics.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DeleteParams {
    /// Resource UUID
    pub id: String,
    /// Permanently delete instead of moving to the trashcan (default false)
    pub ultimate: Option<bool>,
}

impl DeleteParams {
    pub fn to_query(&self) -> Vec<(&'static str, String)> {
        match self.ultimate {
            Some(ultimate) => vec![("ultimate", ultimate.to_string())],
            None => Vec::new(),
        }
    }
}

/// Render a serializable value as a successful JSON tool result.
pub fn json_result(value: &impl Serialize) -> Result<CallToolResult, McpError> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|err| McpError::internal_error(err.to_string(), None))?;
    Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
}

/// Builder for gateway JSON request bodies. Tool arguments are snake_case;
/// gateway bodies are camelCase — every `set*` call names the gateway key
/// explicitly, and unset optionals are omitted entirely so the gateway's
/// "absent means unchanged" semantics hold (and its `deny_unknown_fields`
/// bodies never see a key they do not expect).
#[derive(Debug, Default)]
pub struct Body(serde_json::Map<String, serde_json::Value>);

impl Body {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(mut self, key: &str, value: impl Into<serde_json::Value>) -> Self {
        self.0.insert(key.to_string(), value.into());
        self
    }

    pub fn set_opt(self, key: &str, value: Option<impl Into<serde_json::Value>>) -> Self {
        match value {
            Some(value) => self.set(key, value),
            None => self,
        }
    }

    /// Set a key from any serializable value (maps, nested objects).
    pub fn set_opt_json<T: Serialize>(self, key: &str, value: Option<T>) -> Self {
        match value {
            Some(value) => {
                let value = serde_json::to_value(value).expect("tool arguments serialize");
                self.set(key, value)
            }
            None => self,
        }
    }

    pub fn into_value(self) -> serde_json::Value {
        serde_json::Value::Object(self.0)
    }
}

/// Generic create tool body: POST and return the gateway's `{id}` envelope.
pub async fn create_resource(
    gateway: &GatewayClient,
    resource: &str,
    body: Body,
    stage: &str,
) -> Result<CallToolResult, McpError> {
    create_resource_at(gateway, &[resource], body, stage).await
}

/// [`create_resource`] for nested collection paths.
pub async fn create_resource_at(
    gateway: &GatewayClient,
    segments: &[&str],
    body: Body,
    stage: &str,
) -> Result<CallToolResult, McpError> {
    match gateway
        .post_json::<serde_json::Value>(segments, &body.into_value())
        .await
    {
        Ok(created) => json_result(&created),
        Err(err) => Ok(gateway_tool_error(stage, &err)),
    }
}

/// Generic clone tool body: bodyless POST to `.../{id}/clone`, returning
/// the gateway's `{id}` envelope for the copy.
pub async fn clone_resource(
    gateway: &GatewayClient,
    resource: &str,
    id: &str,
    stage: &str,
) -> Result<CallToolResult, McpError> {
    match gateway
        .post_action::<serde_json::Value>(&[resource, id, "clone"])
        .await
    {
        Ok(created) => json_result(&created),
        Err(err) => Ok(gateway_tool_error(stage, &err)),
    }
}

/// Generic update tool body: PUT and pass the updated resource through.
pub async fn update_resource(
    gateway: &GatewayClient,
    segments: &[&str],
    body: Body,
    stage: &str,
) -> Result<CallToolResult, McpError> {
    match gateway
        .put_json::<serde_json::Value>(segments, &body.into_value())
        .await
    {
        Ok(updated) => json_result(&updated),
        Err(err) => Ok(gateway_tool_error(stage, &err)),
    }
}

/// Update tool body for PUTs that answer 204: reports `{updated: true}`
/// plus the addressed path segments.
pub async fn update_resource_empty(
    gateway: &GatewayClient,
    segments: &[&str],
    body: Body,
    stage: &str,
) -> Result<CallToolResult, McpError> {
    match gateway.put_json_empty(segments, &body.into_value()).await {
        Ok(()) => json_result(&serde_json::json!({ "updated": true, "path": segments.join("/") })),
        Err(err) => Ok(gateway_tool_error(stage, &err)),
    }
}

/// Generic delete tool body (204 expected; `ultimate` skips the trashcan).
pub async fn delete_resource(
    gateway: &GatewayClient,
    resource: &str,
    params: &DeleteParams,
    stage: &str,
) -> Result<CallToolResult, McpError> {
    match gateway
        .delete(&[resource, &params.id], &params.to_query())
        .await
    {
        Ok(()) => json_result(&serde_json::json!({ "deleted": true, "id": params.id })),
        Err(err) => Ok(gateway_tool_error(stage, &err)),
    }
}

/// Delete tool body for resources whose gateway route has no `ultimate`
/// option (reports, audits, policies, hosts, operating systems, generic
/// assets, agents). Sending `ultimate` to those routes is rejected by the
/// gateway, so the parameter is not even offered.
pub async fn delete_resource_plain(
    gateway: &GatewayClient,
    resource: &str,
    id: &str,
    stage: &str,
) -> Result<CallToolResult, McpError> {
    match gateway.delete(&[resource, id], &[]).await {
        Ok(()) => json_result(&serde_json::json!({ "deleted": true, "id": id })),
        Err(err) => Ok(gateway_tool_error(stage, &err)),
    }
}

/// Copy only `keys` (that are present and non-null) out of a JSON object.
/// Used to summarize gateway list rows: the key sets come from the gateway
/// spec, the summaries keep tool output within an LLM's token budget.
pub fn pick(value: &serde_json::Value, keys: &[&str]) -> serde_json::Value {
    let mut out = serde_json::Map::new();
    if let Some(object) = value.as_object() {
        for key in keys {
            if let Some(v) = object.get(*key)
                && !v.is_null()
            {
                out.insert((*key).to_string(), v.clone());
            }
        }
    }
    serde_json::Value::Object(out)
}

/// Generic list tool body: fetch `{data, pagination}` from the gateway,
/// summarize each row down to `keys`, and return `{<out_key>, pagination}`.
pub async fn list_summarized(
    gateway: &GatewayClient,
    resource: &str,
    out_key: &str,
    keys: &[&str],
    params: &ListParams,
    stage: &str,
) -> Result<CallToolResult, McpError> {
    list_summarized_at(gateway, &[resource], out_key, keys, params, stage).await
}

/// [`list_summarized`] for nested paths (e.g. report drill-down pages).
pub async fn list_summarized_at(
    gateway: &GatewayClient,
    segments: &[&str],
    out_key: &str,
    keys: &[&str],
    params: &ListParams,
    stage: &str,
) -> Result<CallToolResult, McpError> {
    list_summarized_query(gateway, segments, &params.to_query(), out_key, keys, stage).await
}

/// Lowest-level list helper: arbitrary path and query, summarized rows.
/// Any non-`data` envelope fields (e.g. the feed access flags) are passed
/// through alongside the summarized rows and the pagination block.
pub async fn list_summarized_query(
    gateway: &GatewayClient,
    segments: &[&str],
    query: &[(&str, String)],
    out_key: &str,
    keys: &[&str],
    stage: &str,
) -> Result<CallToolResult, McpError> {
    let value: serde_json::Value = match gateway.get_json_query(segments, query).await {
        Ok(value) => value,
        Err(err) => return Ok(gateway_tool_error(stage, &err)),
    };

    let rows: Vec<serde_json::Value> = value["data"]
        .as_array()
        .map(|data| data.iter().map(|row| pick(row, keys)).collect())
        .unwrap_or_default();

    let mut out = serde_json::Map::new();
    out.insert(out_key.to_string(), serde_json::Value::Array(rows));
    if let Some(envelope) = value.as_object() {
        for (key, extra) in envelope {
            if key != "data" && !extra.is_null() {
                out.insert(key.clone(), extra.clone());
            }
        }
    }
    json_result(&serde_json::Value::Object(out))
}

/// Generic get tool body: fetch one resource and pass the gateway's JSON
/// through unchanged (the gateway spec is the contract; full detail is the
/// point of a get tool).
pub async fn get_passthrough(
    gateway: &GatewayClient,
    segments: &[&str],
    stage: &str,
) -> Result<CallToolResult, McpError> {
    get_passthrough_query(gateway, segments, &[], stage).await
}

/// [`get_passthrough`] with query parameters (type-scoped assets, paged
/// report detail, NVT-scoped preferences).
pub async fn get_passthrough_query(
    gateway: &GatewayClient,
    segments: &[&str],
    query: &[(&str, String)],
    stage: &str,
) -> Result<CallToolResult, McpError> {
    match gateway
        .get_json_query::<serde_json::Value>(segments, query)
        .await
    {
        Ok(value) => json_result(&value),
        Err(err) => Ok(gateway_tool_error(stage, &err)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_params_map_to_gateway_query_names() {
        let params = ListParams {
            filter: Some("severity>5".into()),
            filter_id: None,
            page: Some(2),
            per_page: Some(50),
        };
        assert_eq!(
            params.to_query(),
            vec![
                ("filter", "severity>5".to_string()),
                ("page", "2".to_string()),
                ("perPage", "50".to_string()),
            ]
        );
    }

    #[test]
    fn empty_list_params_produce_no_query() {
        assert!(ListParams::default().to_query().is_empty());
    }

    #[test]
    fn trash_and_page_params_add_only_their_keys() {
        let trash = TrashListParams {
            list: ListParams::default(),
            trash: Some(true),
        };
        assert_eq!(trash.to_query(), vec![("trash", "true".to_string())]);

        let page = PageParams {
            page: Some(3),
            per_page: None,
        };
        assert_eq!(page.to_query(), vec![("page", "3".to_string())]);

        let filter = FilterParams {
            filter: None,
            filter_id: Some("f-1".into()),
        };
        assert_eq!(filter.to_query(), vec![("filterId", "f-1".to_string())]);
    }

    #[test]
    fn delete_params_only_send_ultimate_when_set() {
        let plain = DeleteParams {
            id: "x".into(),
            ultimate: None,
        };
        assert!(plain.to_query().is_empty());

        let ultimate = DeleteParams {
            id: "x".into(),
            ultimate: Some(true),
        };
        assert_eq!(ultimate.to_query(), vec![("ultimate", "true".to_string())]);
    }

    #[test]
    fn body_omits_unset_and_serializes_nested_values() {
        let body = Body::new()
            .set("name", "x")
            .set_opt("comment", None::<String>)
            .set_opt_json(
                "preferences",
                Some(std::collections::BTreeMap::from([("k", "v")])),
            )
            .into_value();
        assert_eq!(
            body,
            serde_json::json!({"name": "x", "preferences": {"k": "v"}})
        );
    }
}
