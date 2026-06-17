//! Shapes every MCP tool result into the "summary + structuredContent" form.
//!
//! Success → one text block (summary + trimmed JSON fence) plus full-fidelity
//! `structured_content` (with `suggested_next_actions`). Failure → an
//! `is_error: true` result carrying a shared error envelope.

use crate::error::ClientError;
use gauge_query::{QueryRequest, TimeRange};
use rmcp::model::{CallToolResult, Content};
use serde_json::{Value, json};

/// A suggested follow-up. `tool: None` describes a user action (e.g. "run `gauge login`").
#[derive(Debug, Clone)]
pub struct NextAction {
    pub description: String,
    pub tool: Option<&'static str>,
    pub arguments: Option<Value>,
}

impl NextAction {
    pub fn call(description: impl Into<String>, tool: &'static str, arguments: Value) -> Self {
        Self {
            description: description.into(),
            tool: Some(tool),
            arguments: Some(arguments),
        }
    }
    pub fn user(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            tool: None,
            arguments: None,
        }
    }
    fn to_value(&self) -> Value {
        let mut o = json!({ "description": self.description });
        if let Some(t) = self.tool {
            o["tool"] = json!(t);
        }
        if let Some(a) = &self.arguments {
            o["arguments"] = a.clone();
        }
        o
    }
}

fn actions_value(actions: &[NextAction]) -> Value {
    Value::Array(actions.iter().map(NextAction::to_value).collect())
}

/// A successful tool result, pre-render.
pub struct ToolOutcome {
    pub summary: String,
    pub trimmed: Value,
    pub structured: Value,
    pub next_actions: Vec<NextAction>,
}

impl ToolOutcome {
    pub fn into_result(self) -> CallToolResult {
        let mut structured = self.structured;
        if let Value::Object(map) = &mut structured {
            map.insert(
                "suggested_next_actions".to_owned(),
                actions_value(&self.next_actions),
            );
        }
        let trimmed = serde_json::to_string(&self.trimmed).unwrap_or_else(|_| "{}".to_owned());
        let text = format!("{}\n\n```json\n{trimmed}\n```", self.summary);
        let mut result = CallToolResult::success(vec![Content::text(text)]);
        result.structured_content = Some(structured);
        result
    }
}

/// Closed set of tool-execution error kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Unauthenticated,
    InvalidInput,
    NotFound,
    RateLimited,
    CloudError,
    Internal,
}

impl ErrorKind {
    pub fn code(self) -> &'static str {
        match self {
            ErrorKind::Unauthenticated => "UNAUTHENTICATED",
            ErrorKind::InvalidInput => "INVALID_INPUT",
            ErrorKind::NotFound => "NOT_FOUND",
            ErrorKind::RateLimited => "RATE_LIMITED",
            ErrorKind::CloudError => "CLOUD_ERROR",
            ErrorKind::Internal => "INTERNAL",
        }
    }
    /// `false` means an identical retry cannot succeed — recovery needs a different call.
    pub fn retryable(self) -> bool {
        matches!(
            self,
            ErrorKind::InvalidInput | ErrorKind::RateLimited | ErrorKind::CloudError
        )
    }
}

/// A tool-execution failure, pre-render (becomes an `is_error: true` result).
pub struct ToolFailure {
    pub kind: ErrorKind,
    pub message: String,
    pub guidance: String,
    pub details: Value,
    pub next_actions: Vec<NextAction>,
}

impl ToolFailure {
    pub fn new(kind: ErrorKind, message: impl Into<String>, guidance: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            guidance: guidance.into(),
            details: Value::Null,
            next_actions: Vec::new(),
        }
    }

    pub fn with_actions(mut self, actions: Vec<NextAction>) -> Self {
        self.next_actions = actions;
        self
    }

    fn discover_meta_action() -> NextAction {
        NextAction::call(
            "Discover queryable apps, event names, and attribute keys",
            "get_meta",
            json!({}),
        )
    }

    pub fn from_client_error(e: ClientError) -> Self {
        let msg = e.to_string();
        match e {
            ClientError::Auth(_)
            | ClientError::KeyMissing(_)
            | ClientError::KeyExists(_)
            | ClientError::ConfigMissing(_)
            | ClientError::ConfigInvalid(_)
            | ClientError::NoConfigDir => ToolFailure::new(
                ErrorKind::Unauthenticated,
                msg,
                "Authentication or local config is not set up.",
            )
            .with_actions(vec![NextAction::user(
                "Ask the user to run `gauge login` (and `gauge keys generate` if no key exists).",
            )]),
            ClientError::Api {
                status,
                code,
                message,
                remediation,
            } => {
                let kind = match status {
                    400 => ErrorKind::InvalidInput,
                    401 | 403 => ErrorKind::Unauthenticated,
                    404 => ErrorKind::NotFound,
                    429 => ErrorKind::RateLimited,
                    _ => ErrorKind::CloudError,
                };
                let guidance = remediation.unwrap_or_else(|| {
                    match kind {
                        ErrorKind::InvalidInput => "Fix the request arguments and retry; call get_meta to discover valid apps and event names.",
                        ErrorKind::NotFound => "Re-derive identifiers from get_meta before retrying.",
                        ErrorKind::RateLimited => "Rate limit hit — wait for the window to reset, then retry.",
                        ErrorKind::Unauthenticated => "Re-authenticate (`gauge login`) and retry.",
                        _ => "The server returned an error; retry shortly.",
                    }
                    .to_owned()
                });
                let mut f = ToolFailure::new(kind, message, guidance);
                f.details = json!({ "server_code": code, "status": status });
                if matches!(kind, ErrorKind::InvalidInput | ErrorKind::NotFound) {
                    f = f.with_actions(vec![Self::discover_meta_action()]);
                }
                f
            }
            ClientError::Http(m) => ToolFailure::new(
                ErrorKind::CloudError,
                format!("http error: {m}"),
                "Could not reach the gauge server; retry shortly.",
            ),
            ClientError::Io(_) | ClientError::Json(_) => ToolFailure::new(
                ErrorKind::Internal,
                msg,
                "An internal error occurred while processing the response.",
            ),
        }
    }

    pub fn into_result(self) -> CallToolResult {
        let mut error = json!({
            "code": self.kind.code(),
            "retryable": self.kind.retryable(),
            "message": self.message,
        });
        if let (Value::Object(emap), Value::Object(dmap)) = (&mut error, &self.details) {
            for (k, v) in dmap {
                emap.insert(k.clone(), v.clone());
            }
        }
        let structured =
            json!({ "error": error, "suggested_next_actions": actions_value(&self.next_actions) });
        let trimmed = serde_json::to_string(
            &json!({ "error": { "code": self.kind.code(), "retryable": self.kind.retryable() } }),
        )
        .unwrap_or_else(|_| "{}".to_owned());
        let text = format!("{}\n\n```json\n{trimmed}\n```", self.guidance);
        let mut result = CallToolResult::error(vec![Content::text(text)]);
        result.structured_content = Some(structured);
        result
    }
}

/// Up to this many rows go in the text fence; the full set is always in structured_content.
const FENCE_ROW_CAP: usize = 3;

fn rows_of(resp: &Value) -> &[Value] {
    resp.get("rows")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// `get_meta` -> MetaResponse `{ apps: [AppMeta..] }`.
pub fn project_meta(resp: &Value) -> ToolOutcome {
    let empty = vec![];
    let apps = resp.get("apps").and_then(Value::as_array).unwrap_or(&empty);
    let names: Vec<&str> = apps
        .iter()
        .filter_map(|a| a.get("app").and_then(Value::as_str))
        .collect();
    let total_events: i64 = apps
        .iter()
        .filter_map(|a| a.get("total_events").and_then(Value::as_i64))
        .sum();
    let event_name_count: usize = apps
        .iter()
        .filter_map(|a| a.get("event_names").and_then(Value::as_array).map(Vec::len))
        .sum();
    let summary = format!(
        "Apps: {}. {} event names, {} events total.",
        if names.is_empty() {
            "(none)".to_owned()
        } else {
            names.join(", ")
        },
        event_name_count,
        total_events,
    );
    let mut next_actions = Vec::new();
    if let Some(top_app) = names.first() {
        next_actions.push(NextAction::call(
            format!("See the most-used events for {top_app} (last 30 days)"),
            "top_events",
            json!({ "period": "30d", "app": top_app }),
        ));
        // A ready, valid query example using a real app + (if any) a real event name.
        let event = apps
            .first()
            .and_then(|a| a.get("event_names"))
            .and_then(Value::as_array)
            .and_then(|e| e.first())
            .and_then(Value::as_str);
        let mut args = json!({ "period": "7d", "app": top_app });
        if let Some(ev) = event {
            args["event_name"] = json!(ev);
        }
        next_actions.push(NextAction::call(
            "Count unique users for a concrete app/event in the last 7 days",
            "unique_users",
            args,
        ));
    }
    let trimmed = json!({
        "apps": names,
        "event_name_count": event_name_count,
        "total_events": total_events,
    });
    ToolOutcome {
        summary,
        trimmed,
        structured: resp.clone(),
        next_actions,
    }
}

/// Generic query result `{ rows, truncated, elapsed_ms }` for `query_telemetry`.
pub fn project_query(resp: &Value, req: &QueryRequest) -> ToolOutcome {
    let rows = rows_of(resp);
    let truncated = resp
        .get("truncated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let elapsed = resp.get("elapsed_ms").and_then(Value::as_u64).unwrap_or(0);
    let summary = format!("{} rows, {elapsed}ms (truncated: {truncated}).", rows.len());
    let mut next_actions = vec![NextAction::call(
        "Discover queryable apps, event names, and attribute keys",
        "get_meta",
        json!({}),
    )];
    // No granularity + a relative range -> offer a day-bucketed trend over the same period.
    if req.granularity.is_none()
        && let TimeRange::Last { last } = &req.time_range
    {
        next_actions.push(NextAction::call(
            "View this as a day-by-day trend over the same period",
            "events_over_time",
            json!({ "period": last, "granularity": "day" }),
        ));
    }
    let trimmed = json!({
        "rows": rows.iter().take(FENCE_ROW_CAP).cloned().collect::<Vec<_>>(),
        "row_count": rows.len(),
        "truncated": truncated,
    });
    ToolOutcome {
        summary,
        trimmed,
        structured: resp.clone(),
        next_actions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::error::ClientError;

    #[test]
    fn client_errors_map_to_codes() {
        let cases = [
            (ClientError::NoConfigDir, "UNAUTHENTICATED", false),
            (ClientError::Http("boom".into()), "CLOUD_ERROR", true),
            (
                ClientError::Api {
                    status: 400,
                    code: "bad".into(),
                    message: "nope".into(),
                    remediation: None,
                },
                "INVALID_INPUT",
                true,
            ),
            (
                ClientError::Api {
                    status: 404,
                    code: "missing".into(),
                    message: "nope".into(),
                    remediation: None,
                },
                "NOT_FOUND",
                false,
            ),
            (
                ClientError::Api {
                    status: 429,
                    code: "slow".into(),
                    message: "nope".into(),
                    remediation: None,
                },
                "RATE_LIMITED",
                true,
            ),
            (
                ClientError::Api {
                    status: 503,
                    code: "down".into(),
                    message: "nope".into(),
                    remediation: None,
                },
                "CLOUD_ERROR",
                true,
            ),
        ];
        for (err, code, retryable) in cases {
            let f = ToolFailure::from_client_error(err);
            assert_eq!(f.kind.code(), code);
            assert_eq!(f.kind.retryable(), retryable);
            let result = f.into_result();
            assert_eq!(result.is_error, Some(true));
            let sc = result.structured_content.unwrap();
            assert_eq!(sc["error"]["code"], json!(code));
            assert_eq!(sc["error"]["retryable"], json!(retryable));
        }
    }

    use gauge_query::{QueryRequest, TimeRange};

    #[test]
    fn project_meta_summarizes_and_suggests_drill() {
        let resp = json!({
            "apps": [
                { "app": "tome", "event_names": ["tome.search", "tome.open"], "attribute_keys": ["lang"], "total_events": 1200, "first_event": "2026-05-01T00:00:00Z", "last_event": "2026-06-01T00:00:00Z" },
                { "app": "midnight-manual", "event_names": ["mm.search"], "attribute_keys": [], "total_events": 50, "first_event": null, "last_event": null }
            ]
        });
        let o = project_meta(&resp);
        assert!(o.summary.contains("tome"));
        assert!(o.summary.contains("midnight-manual"));
        // a drill action carries a real app name
        let drill = o
            .next_actions
            .iter()
            .find(|a| a.tool == Some("top_events"))
            .unwrap();
        assert_eq!(drill.arguments.as_ref().unwrap()["app"], json!("tome"));
    }

    #[test]
    fn project_query_reports_rowcount_and_trend_action() {
        let resp =
            json!({ "rows": [ {"count": 5}, {"count": 3} ], "truncated": false, "elapsed_ms": 11 });
        let req = QueryRequest {
            measures: vec![gauge_query::Measure::Count],
            dimensions: vec![],
            filters: vec![],
            time_range: TimeRange::Last { last: "7d".into() },
            granularity: None,
            order: vec![],
            limit: None,
        };
        let o = project_query(&resp, &req);
        assert!(o.summary.contains("2 rows"));
        // no granularity + Last range → an events_over_time trend action with the same period
        let trend = o
            .next_actions
            .iter()
            .find(|a| a.tool == Some("events_over_time"))
            .unwrap();
        assert_eq!(trend.arguments.as_ref().unwrap()["period"], json!("7d"));
    }

    #[test]
    fn outcome_renders_summary_fence_and_structured() {
        let outcome = ToolOutcome {
            summary: "2 rows, 9ms.".to_owned(),
            trimmed: json!({ "rows": 2 }),
            structured: json!({ "rows": [], "truncated": false, "elapsed_ms": 9 }),
            next_actions: vec![NextAction::call("Discover schema", "get_meta", json!({}))],
        };
        let result = outcome.into_result();
        assert_eq!(result.is_error, Some(false));
        // text block: summary then a ```json fence. `Content::as_text()` returns
        // `Option<&RawTextContent>`; `.text` is the string field.
        let text = result.content[0]
            .as_text()
            .expect("text content")
            .text
            .clone();
        assert!(text.starts_with("2 rows, 9ms."));
        assert!(text.contains("```json"));
        // structured_content carries the payload + injected suggested_next_actions
        let sc = result
            .structured_content
            .expect("structured_content present");
        assert_eq!(sc["truncated"], json!(false));
        assert_eq!(sc["suggested_next_actions"][0]["tool"], json!("get_meta"));
    }
}
