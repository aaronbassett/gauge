use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, ServerCapabilities, ServerInfo};
use rmcp::{ErrorData as McpError, ServerHandler, ServiceExt, tool, tool_handler, tool_router};

use crate::api::ApiClient;
use crate::error::ClientError;
use crate::mcp::tools::{
    EventsOverTimeParams, TopEventsParams, UniqueUsersParams, events_over_time_query,
    top_events_query, unique_users_query,
};

#[derive(Clone)]
pub struct GaugeMcp {
    api: Arc<ApiClient>,
    tool_router: ToolRouter<Self>,
}

fn to_mcp_err(e: ClientError) -> McpError {
    // ClientError Display already carries remediation ("run `gauge login`" etc.)
    McpError::internal_error(e.to_string(), None)
}

fn ok_json<T: serde::Serialize>(v: &T) -> Result<CallToolResult, McpError> {
    let json = serde_json::to_string_pretty(v)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}

#[tool_router]
impl GaugeMcp {
    pub fn new(api: Arc<ApiClient>) -> Self {
        Self { api, tool_router: Self::tool_router() }
    }

    #[tool(description = "Run an analytics query over anonymous telemetry events. Measures: count, unique_installs, unique_sessions. Dimensions: app, event_name, app_version, os, arch, attr.<key>. Time ranges: {\"last\":\"7d\"} or RFC3339 from/to. Use get_meta first to discover apps, event names, and attribute keys.")]
    pub async fn query_telemetry(
        &self,
        Parameters(req): Parameters<gauge_query::QueryRequest>,
    ) -> Result<CallToolResult, McpError> {
        ok_json(&self.api.query(&req).await.map_err(to_mcp_err)?)
    }

    #[tool(description = "Discover what is queryable: apps, their event names, attribute keys, totals, and time span.")]
    pub async fn get_meta(&self) -> Result<CallToolResult, McpError> {
        ok_json(&self.api.meta().await.map_err(to_mcp_err)?)
    }

    #[tool(description = "How many unique users (anonymous installs) in a period, optionally filtered by app and/or event name. Example: unique users who ran a search in the last week.")]
    pub async fn unique_users(
        &self,
        Parameters(p): Parameters<UniqueUsersParams>,
    ) -> Result<CallToolResult, McpError> {
        ok_json(&self.api.query(&unique_users_query(&p)).await.map_err(to_mcp_err)?)
    }

    #[tool(description = "The most used events (top-N event types) in a period, ranked by count or unique installs. Answers 'what is our most used X'.")]
    pub async fn top_events(
        &self,
        Parameters(p): Parameters<TopEventsParams>,
    ) -> Result<CallToolResult, McpError> {
        ok_json(&self.api.query(&top_events_query(&p)).await.map_err(to_mcp_err)?)
    }

    #[tool(description = "Event volume over time (hour/day/week buckets) for trend questions.")]
    pub async fn events_over_time(
        &self,
        Parameters(p): Parameters<EventsOverTimeParams>,
    ) -> Result<CallToolResult, McpError> {
        ok_json(&self.api.query(&events_over_time_query(&p)).await.map_err(to_mcp_err)?)
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for GaugeMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "Query anonymous product telemetry for Midnight/DevRel apps (Tome, Midnight Manual). \
             Start with get_meta to see what exists. Telemetry is anonymous: there is no way to \
             query individual users — only aggregate counts and unique-install counts.",
        )
    }
}

pub async fn serve(api: Arc<ApiClient>) -> Result<(), Box<dyn std::error::Error>> {
    let service = GaugeMcp::new(api).serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
