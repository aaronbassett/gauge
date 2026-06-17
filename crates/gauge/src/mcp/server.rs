use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ListToolsResult, PaginatedRequestParams,
    ServerCapabilities, ServerInfo,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ErrorData as McpError, ServerHandler, ServiceExt, tool, tool_router};
use serde_json::{Value, json};

use crate::api::ApiClient;
use crate::mcp::render::{
    ErrorKind, NextAction, ToolFailure, project_events_over_time, project_meta, project_query,
    project_top_events, project_unique_users,
};
use crate::mcp::schemas::apply_output_schemas;
use crate::mcp::tools::{
    EventsOverTimeParams, TopEventsParams, UniqueUsersParams, events_over_time_query,
    top_events_query, unique_users_query,
};

#[derive(Clone)]
pub struct GaugeMcp {
    api: Arc<ApiClient>,
    tool_router: ToolRouter<Self>,
}

impl GaugeMcp {
    /// Run a query and convert the typed response into a JSON `Value` for the
    /// projectors, mapping any client error into a `ToolFailure`.
    async fn query_to_value(&self, req: &gauge_query::QueryRequest) -> Result<Value, ToolFailure> {
        self.api
            .query(req)
            .await
            .map(|r| serde_json::to_value(&r).unwrap_or_default())
            .map_err(ToolFailure::from_client_error)
    }
}

#[tool_router]
impl GaugeMcp {
    pub fn new(api: Arc<ApiClient>) -> Self {
        Self {
            api,
            tool_router: Self::tool_router(),
        }
    }

    /// Run an analytics query over anonymous telemetry events. Measures: count, unique_installs, unique_sessions. Dimensions: app, event_name, app_version, os, arch, attr.<key>. Time ranges: {"last":"7d"} or RFC3339 from/to. Use get_meta first to discover apps, event names, and attribute keys.
    #[tool(annotations(
        title = "Query telemetry",
        read_only_hint = true,
        open_world_hint = false
    ))]
    pub async fn query_telemetry(
        &self,
        Parameters(req): Parameters<gauge_query::QueryRequest>,
    ) -> Result<CallToolResult, McpError> {
        if let Err(e) = gauge_query::validate(&req) {
            return Ok(ToolFailure::new(
                ErrorKind::InvalidInput,
                e.to_string(),
                "The query failed validation; fix the named field. Call get_meta to discover valid values.",
            )
            .with_actions(vec![NextAction::call(
                "Discover queryable apps, event names, and attribute keys",
                "get_meta",
                json!({}),
            )])
            .into_result());
        }
        Ok(match self.query_to_value(&req).await {
            Ok(v) => project_query(&v, &req).into_result(),
            Err(f) => f.into_result(),
        })
    }

    /// Discover what is queryable: apps, their event names, attribute keys, totals, and time span.
    #[tool(annotations(
        title = "Discover schema",
        read_only_hint = true,
        open_world_hint = false
    ))]
    pub async fn get_meta(&self) -> Result<CallToolResult, McpError> {
        Ok(match self.api.meta().await {
            Ok(m) => project_meta(&serde_json::to_value(&m).unwrap_or_default()).into_result(),
            Err(e) => ToolFailure::from_client_error(e).into_result(),
        })
    }

    /// How many unique users (anonymous installs) in a period, optionally filtered by app and/or event name. Example: unique users who ran a search in the last week.
    #[tool(annotations(title = "Unique users", read_only_hint = true, open_world_hint = false))]
    pub async fn unique_users(
        &self,
        Parameters(p): Parameters<UniqueUsersParams>,
    ) -> Result<CallToolResult, McpError> {
        let req = unique_users_query(&p);
        Ok(match self.query_to_value(&req).await {
            Ok(v) => project_unique_users(&v, &p).into_result(),
            Err(f) => f.into_result(),
        })
    }

    /// The most used events (top-N event types) in a period, ranked by count or unique installs. Answers 'what is our most used X'.
    #[tool(annotations(title = "Top events", read_only_hint = true, open_world_hint = false))]
    pub async fn top_events(
        &self,
        Parameters(p): Parameters<TopEventsParams>,
    ) -> Result<CallToolResult, McpError> {
        let req = top_events_query(&p);
        Ok(match self.query_to_value(&req).await {
            Ok(v) => project_top_events(&v, &p).into_result(),
            Err(f) => f.into_result(),
        })
    }

    /// Event volume over time (hour/day/week buckets) for trend questions.
    #[tool(annotations(
        title = "Events over time",
        read_only_hint = true,
        open_world_hint = false
    ))]
    pub async fn events_over_time(
        &self,
        Parameters(p): Parameters<EventsOverTimeParams>,
    ) -> Result<CallToolResult, McpError> {
        let req = events_over_time_query(&p);
        Ok(match self.query_to_value(&req).await {
            Ok(v) => project_events_over_time(&v, &p).into_result(),
            Err(f) => f.into_result(),
        })
    }
}

impl ServerHandler for GaugeMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "Query anonymous product telemetry for Midnight/DevRel apps (Tome, Midnight Manual). \
             Start with get_meta to see what exists. Telemetry is anonymous: there is no way to \
             query individual users — only aggregate counts and unique-install counts.",
        )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let mut tools = self.tool_router.list_all();
        apply_output_schemas(&mut tools);
        Ok(ListToolsResult {
            tools,
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let ctx = ToolCallContext::new(self, request, context);
        self.tool_router.call(ctx).await
    }
}

pub async fn serve(api: Arc<ApiClient>) -> Result<(), Box<dyn std::error::Error>> {
    let service = GaugeMcp::new(api).serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_list_has_annotations_and_output_schemas() {
        let mut tools = GaugeMcp::tool_router().list_all();
        apply_output_schemas(&mut tools);

        assert_eq!(tools.len(), 5, "expected 5 MCP tools, got {}", tools.len());

        // Every tool is annotated read-only.
        for t in &tools {
            let ann = t
                .annotations
                .as_ref()
                .unwrap_or_else(|| panic!("{} missing annotations", t.name));
            assert_eq!(
                ann.read_only_hint,
                Some(true),
                "{} should be read-only",
                t.name
            );
        }

        // Every tool advertises an output schema with described properties (not an
        // empty passthrough).
        for name in [
            "query_telemetry",
            "get_meta",
            "unique_users",
            "top_events",
            "events_over_time",
        ] {
            let t = tools
                .iter()
                .find(|t| t.name.as_ref() == name)
                .unwrap_or_else(|| panic!("missing tool {name}"));
            let schema = t
                .output_schema
                .as_ref()
                .unwrap_or_else(|| panic!("{name} missing output_schema"));
            assert!(
                schema.get("properties").is_some(),
                "{name} output_schema has no properties"
            );
        }
    }
}
