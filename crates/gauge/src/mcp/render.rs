//! Shapes every MCP tool result into the "summary + structuredContent" form.
//!
//! Success → one text block (summary + trimmed JSON fence) plus full-fidelity
//! `structured_content` (with `suggested_next_actions`). Failure → an
//! `is_error: true` result carrying a shared error envelope.

use crate::error::ClientError;
use crate::mcp::tools::{
    EventsOverTimeParams, NumericHistogramParams, NumericStatsParams, TopBy, TopEventsParams,
    UniqueUsersParams,
};
use gauge_query::{Field, FilterOp, FilterValue, QueryRequest, TimeRange};
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
        matches!(self, ErrorKind::RateLimited | ErrorKind::CloudError)
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
    // No granularity + a relative range -> offer a day-bucketed trend over the same
    // period, carrying forward any app/event_name equality filter so the suggested
    // trend keeps the original scope.
    // Only suggest this for plain count-style queries — not for aggregate/bucket queries.
    // req.measures is guaranteed non-empty by validate(); `all` is not vacuously true here.
    let is_plain = req.measures.iter().all(|m| m.numeric_field().is_none())
        && !req
            .dimensions
            .iter()
            .any(|d| matches!(d, gauge_query::Dimension::Bucket { .. }));
    if is_plain
        && req.granularity.is_none()
        && let TimeRange::Last { last } = &req.time_range
    {
        let mut args = json!({ "period": last, "granularity": "day" });
        for f in &req.filters {
            if matches!(f.op, FilterOp::Eq)
                && let Some(FilterValue::One(v)) = &f.value
            {
                match &f.field {
                    Field::App => args["app"] = json!(v),
                    Field::EventName => args["event_name"] = json!(v),
                    _ => {}
                }
            }
        }
        next_actions.push(NextAction::call(
            "View this as a day-by-day trend over the same period",
            "events_over_time",
            args,
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

/// `unique_users` -> single-row `{ unique_installs: N }`.
pub fn project_unique_users(resp: &Value, p: &UniqueUsersParams) -> ToolOutcome {
    let count = rows_of(resp)
        .first()
        .and_then(|r| r.get("unique_installs"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let scope = match (&p.app, &p.event_name) {
        (Some(a), Some(e)) => format!("{a} · {e}"),
        (Some(a), None) => a.clone(),
        (None, Some(e)) => e.clone(),
        (None, None) => "all apps".to_owned(),
    };
    let summary = format!("{count} unique installs ({scope}, {}).", p.period);
    let next_actions = vec![NextAction::call(
        "See what those users are doing — top events for the same app and period",
        "top_events",
        match &p.app {
            Some(a) => json!({ "period": p.period, "app": a }),
            None => json!({ "period": p.period }),
        },
    )];
    let trimmed = json!({ "unique_installs": count, "scope": scope, "period": p.period });
    ToolOutcome {
        summary,
        trimmed,
        structured: resp.clone(),
        next_actions,
    }
}

/// `top_events` -> rows `{ event_name, <measure> }` ranked desc.
pub fn project_top_events(resp: &Value, p: &TopEventsParams) -> ToolOutcome {
    let measure_key = match p.by.unwrap_or(TopBy::Count) {
        TopBy::Count => "count",
        TopBy::UniqueInstalls => "unique_installs",
    };
    let rows = rows_of(resp);
    let head: Vec<String> = rows
        .iter()
        .take(3)
        .map(|r| {
            let name = r.get("event_name").and_then(Value::as_str).unwrap_or("?");
            let v = r.get(measure_key).and_then(Value::as_i64).unwrap_or(0);
            format!("{name} {v}")
        })
        .collect();
    let summary = format!(
        "Top {} ({}): {}.",
        rows.len().min(3),
        p.period,
        head.join(" · ")
    );
    let mut next_actions = Vec::new();
    if let Some(top) = rows
        .first()
        .and_then(|r| r.get("event_name"))
        .and_then(Value::as_str)
    {
        next_actions.push(NextAction::call(
            format!("Count unique users of {top} over the same period"),
            "unique_users",
            json!({ "period": p.period, "event_name": top }),
        ));
        next_actions.push(NextAction::call(
            format!("Plot {top} volume over time"),
            "events_over_time",
            json!({ "period": p.period, "granularity": "day", "event_name": top }),
        ));
    }
    let trimmed = json!({ "top": rows.iter().take(p.limit.unwrap_or(10) as usize).cloned().collect::<Vec<_>>() });
    ToolOutcome {
        summary,
        trimmed,
        structured: resp.clone(),
        next_actions,
    }
}

/// `numeric_stats` -> single-row `{ avg_*, min_*, max_*, p50_*..p99_* }`.
pub fn project_numeric_stats(resp: &Value, p: &NumericStatsParams) -> ToolOutcome {
    let row = rows_of(resp).first().cloned().unwrap_or_else(|| json!({}));
    let g = |k: &str| row.get(k).and_then(Value::as_f64);
    // Normalize away any "attr." prefix: the server's aggregate aliases are always
    // <agg>_<bare_key> (e.g. "avg_latency_ms"), not "avg_attr.latency_ms".
    let key = p.field.strip_prefix("attr.").unwrap_or(&p.field);
    let num = |o: Option<f64>| o.map(|v| format!("{v:.0}")).unwrap_or_else(|| "n/a".into());
    let scope = match (&p.app, &p.event_name) {
        (Some(a), Some(e)) => format!("{a} · {e}"),
        (Some(a), None) => a.clone(),
        (None, Some(e)) => e.clone(),
        (None, None) => "all apps".to_owned(),
    };
    let summary = format!(
        "{key} ({scope}, {}): avg {}, p95 {}, max {}.",
        p.period,
        num(g(&format!("avg_{key}"))),
        num(g(&format!("p95_{key}"))),
        num(g(&format!("max_{key}")))
    );
    // default edges; the agent can override per field semantics
    let next_actions = vec![NextAction::call(
        format!("See the {key} distribution as a histogram"),
        "numeric_histogram",
        match (&p.app, &p.event_name) {
            (Some(a), Some(e)) => {
                json!({ "period": p.period, "field": key, "app": a, "event_name": e, "edges": [50, 200, 500, 1000] })
            }
            (Some(a), None) => {
                json!({ "period": p.period, "field": key, "app": a, "edges": [50, 200, 500, 1000] })
            }
            (None, Some(e)) => {
                json!({ "period": p.period, "field": key, "event_name": e, "edges": [50, 200, 500, 1000] })
            }
            (None, None) => {
                json!({ "period": p.period, "field": key, "edges": [50, 200, 500, 1000] })
            }
        },
    )];
    ToolOutcome {
        summary,
        trimmed: row,
        structured: resp.clone(),
        next_actions,
    }
}

/// `numeric_histogram` -> rows `{ <bucket_alias>: "<label>", count, unique_installs }`.
pub fn project_numeric_histogram(resp: &Value, p: &NumericHistogramParams) -> ToolOutcome {
    let rows = rows_of(resp);
    let key = &p.field;
    let scope = match (&p.app, &p.event_name) {
        (Some(a), Some(e)) => format!("{a} · {e}"),
        (Some(a), None) => a.clone(),
        (None, Some(e)) => e.clone(),
        (None, None) => "all apps".to_owned(),
    };
    // Resolve the bucket column key: prefer the alias echoed in the response meta, fall back to
    // the canonical "attr.<bare_field>" form that the server always uses as the alias.
    let bucket_col: String = resp
        .get("meta")
        .and_then(|m| m.get("buckets"))
        .and_then(Value::as_array)
        .and_then(|b| b.first())
        .and_then(|b| b.get("alias"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| {
            let bare = p.field.strip_prefix("attr.").unwrap_or(p.field.as_str());
            format!("attr.{bare}")
        });
    // Find the peak bucket by count.
    let peak = rows
        .iter()
        .max_by_key(|r| r.get("count").and_then(Value::as_i64).unwrap_or(0));
    let summary = match peak {
        Some(r) => format!(
            "{key} distribution ({scope}, {}): {} buckets, peak {} in {}.",
            p.period,
            rows.len(),
            r.get("count").and_then(Value::as_i64).unwrap_or(0),
            r.get(bucket_col.as_str())
                .and_then(Value::as_str)
                .unwrap_or("?"),
        ),
        None => format!("{key} distribution ({scope}, {}): 0 buckets.", p.period),
    };
    // Cross-link back to numeric_stats for the same field/scope.
    let stats_args = match (&p.app, &p.event_name) {
        (Some(a), Some(e)) => {
            json!({ "period": p.period, "field": key, "app": a, "event_name": e })
        }
        (Some(a), None) => json!({ "period": p.period, "field": key, "app": a }),
        (None, Some(e)) => json!({ "period": p.period, "field": key, "event_name": e }),
        (None, None) => json!({ "period": p.period, "field": key }),
    };
    let next_actions = vec![NextAction::call(
        format!("See avg/min/max and percentiles for {key} over the same period"),
        "numeric_stats",
        stats_args,
    )];
    // A histogram's distribution shape is the whole point — show up to MAX_BUCKET_EDGES + 1 rows
    // (the full set of possible buckets) rather than the generic FENCE_ROW_CAP (3).
    let hist_row_cap = gauge_query::MAX_BUCKET_EDGES + 1;
    let trimmed = json!({
        "buckets": rows.len(),
        "rows": rows.iter().take(hist_row_cap).cloned().collect::<Vec<_>>(),
    });
    ToolOutcome {
        summary,
        trimmed,
        structured: resp.clone(),
        next_actions,
    }
}

/// `events_over_time` -> rows `{ time_bucket, count }`.
pub fn project_events_over_time(resp: &Value, p: &EventsOverTimeParams) -> ToolOutcome {
    let rows = rows_of(resp);
    let peak = rows
        .iter()
        .max_by_key(|r| r.get("count").and_then(Value::as_i64).unwrap_or(0));
    let summary = match peak {
        Some(r) => format!(
            "{} buckets ({}), peak {} on {}.",
            rows.len(),
            p.period,
            r.get("count").and_then(Value::as_i64).unwrap_or(0),
            r.get("time_bucket").and_then(Value::as_str).unwrap_or("?"),
        ),
        None => format!("0 buckets ({}).", p.period),
    };
    let next_actions = vec![NextAction::call(
        "Break the same window down by event type",
        "top_events",
        match &p.app {
            Some(app) => json!({ "period": p.period, "app": app }),
            None => json!({ "period": p.period }),
        },
    )];
    let trimmed = json!({ "buckets": rows.len(), "rows": rows.iter().take(FENCE_ROW_CAP).cloned().collect::<Vec<_>>() });
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
                false,
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

    use crate::mcp::tools::{EventsOverTimeParams, TopEventsParams, UniqueUsersParams};
    use gauge_query::{QueryRequest, TimeRange};

    #[test]
    fn project_top_events_ranks_and_drills_top() {
        let resp = json!({
            "rows": [
                { "event_name": "tome.search", "count": 1204 },
                { "event_name": "tome.open", "count": 980 }
            ],
            "truncated": false, "elapsed_ms": 8
        });
        let p = TopEventsParams {
            period: "30d".into(),
            app: Some("tome".into()),
            by: None,
            limit: None,
        };
        let o = project_top_events(&resp, &p);
        assert!(o.summary.contains("tome.search"));
        let drill = o
            .next_actions
            .iter()
            .find(|a| a.tool == Some("unique_users"))
            .unwrap();
        assert_eq!(
            drill.arguments.as_ref().unwrap()["event_name"],
            json!("tome.search")
        );
    }

    #[test]
    fn project_unique_users_reads_scalar() {
        let resp =
            json!({ "rows": [ { "unique_installs": 412 } ], "truncated": false, "elapsed_ms": 6 });
        let p = UniqueUsersParams {
            period: "7d".into(),
            app: Some("tome".into()),
            event_name: None,
        };
        let o = project_unique_users(&resp, &p);
        assert!(o.summary.contains("412"));
        assert!(o.next_actions.iter().any(|a| a.tool == Some("top_events")));
    }

    #[test]
    fn project_events_over_time_reports_peak() {
        let resp = json!({
            "rows": [
                { "time_bucket": "2026-06-13", "count": 100 },
                { "time_bucket": "2026-06-14", "count": 312 }
            ],
            "truncated": false, "elapsed_ms": 7
        });
        let p = EventsOverTimeParams {
            period: "7d".into(),
            granularity: gauge_query::Granularity::Day,
            app: None,
            event_name: None,
        };
        let o = project_events_over_time(&resp, &p);
        assert!(o.summary.contains("312"));
        assert!(o.summary.contains("2026-06-14"));
    }

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

    #[test]
    fn project_query_trend_carries_app_filter() {
        use gauge_query::{Field, Filter, FilterOp, FilterValue};
        let resp = json!({ "rows": [], "truncated": false, "elapsed_ms": 4 });
        let req = QueryRequest {
            measures: vec![gauge_query::Measure::Count],
            dimensions: vec![],
            filters: vec![Filter {
                field: Field::App,
                op: FilterOp::Eq,
                value: Some(FilterValue::One("tome".into())),
            }],
            time_range: TimeRange::Last { last: "7d".into() },
            granularity: None,
            order: vec![],
            limit: None,
        };
        let o = project_query(&resp, &req);
        let trend = o
            .next_actions
            .iter()
            .find(|a| a.tool == Some("events_over_time"))
            .unwrap();
        let args = trend.arguments.as_ref().unwrap();
        assert_eq!(args["period"], json!("7d"));
        assert_eq!(args["app"], json!("tome"));
    }

    #[test]
    fn project_top_events_uses_unique_installs_measure() {
        let resp = json!({
            "rows": [ { "event_name": "tome.search", "unique_installs": 88 } ],
            "truncated": false, "elapsed_ms": 5
        });
        let p = TopEventsParams {
            period: "30d".into(),
            app: None,
            by: Some(TopBy::UniqueInstalls),
            limit: None,
        };
        let o = project_top_events(&resp, &p);
        assert!(
            o.summary.contains("88"),
            "summary should read the unique_installs value: {}",
            o.summary
        );
    }

    #[test]
    fn project_meta_handles_zero_apps() {
        let resp = json!({ "apps": [] });
        let o = project_meta(&resp);
        assert!(o.summary.contains("(none)"));
        assert!(o.next_actions.is_empty());
    }

    #[test]
    fn project_query_skips_trend_for_aggregate() {
        use gauge_query::{Field, Measure, QueryRequest, TimeRange};
        let resp =
            json!({ "rows": [ {"avg_latency_ms": 142.0} ], "truncated": false, "elapsed_ms": 5 });
        let req = QueryRequest {
            measures: vec![Measure::Avg(Field::Attr("latency_ms".into()))],
            dimensions: vec![],
            filters: vec![],
            time_range: TimeRange::Last { last: "7d".into() },
            granularity: None,
            order: vec![],
            limit: None,
        };
        let o = project_query(&resp, &req);
        assert!(
            o.next_actions
                .iter()
                .all(|a| a.tool != Some("events_over_time"))
        );
    }

    #[test]
    fn project_query_skips_trend_for_bucket() {
        use gauge_query::{BucketSpec, Dimension, Field, Measure, QueryRequest, TimeRange};
        let resp = json!({ "rows": [ {"bucket": "50-200", "count": 77} ], "truncated": false, "elapsed_ms": 6 });
        let req = QueryRequest {
            measures: vec![Measure::Count],
            dimensions: vec![Dimension::Bucket {
                bucket: BucketSpec {
                    field: Field::Attr("latency_ms".into()),
                    edges: vec![50.0, 200.0],
                },
            }],
            filters: vec![],
            time_range: TimeRange::Last { last: "7d".into() },
            granularity: None,
            order: vec![],
            limit: None,
        };
        let o = project_query(&resp, &req);
        assert!(
            o.next_actions
                .iter()
                .all(|a| a.tool != Some("events_over_time")),
            "bucket dimension should suppress the events_over_time suggestion"
        );
    }

    /// Verifies that the projector reads the bucket column by alias (e.g. "attr.latency_ms"),
    /// not the literal key "bucket". This is the assertion that catches the pre-fix bug where
    /// `r.get("bucket")` always returned `None` and the summary always printed "?".
    #[test]
    fn project_numeric_histogram_names_peak_bucket_label() {
        // Realistic response: rows keyed by "attr.latency_ms" (the server-assigned alias),
        // plus meta that echoes the alias so the projector can resolve the column key.
        let resp = json!({
            "rows": [
                { "attr.latency_ms": "<50",     "count": 120, "unique_installs": 80  },
                { "attr.latency_ms": "50-200",  "count": 450, "unique_installs": 310 },
                { "attr.latency_ms": "200+",    "count": 90,  "unique_installs": 60  }
            ],
            "truncated": false,
            "elapsed_ms": 12,
            "meta": {
                "buckets": [
                    {
                        "field": "attr.latency_ms",
                        "alias": "attr.latency_ms",
                        "edges": [50, 200],
                        "labels": ["<50", "50-200", "200+"]
                    }
                ]
            }
        });
        let p = NumericHistogramParams {
            period: "7d".into(),
            field: "latency_ms".into(),
            edges: vec![50.0, 200.0],
            app: None,
            event_name: None,
        };
        let o = project_numeric_histogram(&resp, &p);

        // Must name the actual peak bucket label ("50-200"), not the fallback "?".
        assert!(
            o.summary.contains("50-200"),
            "summary should name the peak bucket label '50-200', got: {}",
            o.summary
        );
        // Must report the correct count for the peak bucket.
        assert!(
            o.summary.contains("450"),
            "summary should contain the peak count 450, got: {}",
            o.summary
        );
        // next_actions must include a numeric_stats drill-down.
        assert!(
            o.next_actions
                .iter()
                .any(|a| a.tool == Some("numeric_stats")),
            "expected a numeric_stats next action"
        );
    }

    /// Regression: when the agent passes `field: "attr.latency_ms"` the projector must
    /// strip the "attr." prefix before building the aggregate column keys.  Without the
    /// fix the lookup keys would be "avg_attr.latency_ms" etc., which never match the
    /// server-returned "avg_latency_ms", causing the summary to print "n/a" for every
    /// stat.
    #[test]
    fn project_numeric_stats_handles_prefixed_field() {
        // Server always returns bare-key aggregate aliases regardless of how the
        // caller spelled the field.
        let resp = json!({
            "rows": [{
                "avg_latency_ms": 142.0,
                "p95_latency_ms": 480.0,
                "max_latency_ms": 1200.0
            }],
            "truncated": false,
            "elapsed_ms": 9
        });
        let p = NumericStatsParams {
            period: "7d".into(),
            field: "attr.latency_ms".into(), // prefixed — the bug trigger
            app: None,
            event_name: None,
        };
        let o = project_numeric_stats(&resp, &p);
        assert!(
            o.summary.contains("142"),
            "summary should contain avg 142, got: {}",
            o.summary
        );
        assert!(
            o.summary.contains("480"),
            "summary should contain p95 480, got: {}",
            o.summary
        );
        assert!(
            !o.summary.contains("n/a"),
            "summary must not contain 'n/a' when stats are present, got: {}",
            o.summary
        );
    }

    #[test]
    fn project_numeric_histogram_empty_rows_no_panic() {
        let resp = json!({
            "rows": [],
            "truncated": false,
            "elapsed_ms": 5,
            "meta": {
                "buckets": [
                    {
                        "field": "attr.latency_ms",
                        "alias": "attr.latency_ms",
                        "edges": [50, 200],
                        "labels": ["<50", "50-200", "200+"]
                    }
                ]
            }
        });
        let p = NumericHistogramParams {
            period: "7d".into(),
            field: "latency_ms".into(),
            edges: vec![50.0, 200.0],
            app: None,
            event_name: None,
        };
        let o = project_numeric_histogram(&resp, &p);
        // Empty rows should produce a "0 buckets" summary without panicking.
        assert!(
            o.summary.contains("0 buckets"),
            "empty rows should produce a '0 buckets' summary, got: {}",
            o.summary
        );
    }

    #[test]
    fn project_numeric_histogram_next_action_targets_numeric_stats() {
        let resp = json!({
            "rows": [
                { "attr.latency_ms": "50-200", "count": 300, "unique_installs": 200 }
            ],
            "truncated": false,
            "elapsed_ms": 8,
            "meta": {
                "buckets": [
                    {
                        "field": "attr.latency_ms",
                        "alias": "attr.latency_ms",
                        "edges": [50, 200],
                        "labels": ["<50", "50-200", "200+"]
                    }
                ]
            }
        });
        let p = NumericHistogramParams {
            period: "7d".into(),
            field: "latency_ms".into(),
            edges: vec![50.0, 200.0],
            app: Some("tome".into()),
            event_name: Some("tome.search".into()),
        };
        let o = project_numeric_histogram(&resp, &p);
        let stats_action = o
            .next_actions
            .iter()
            .find(|a| a.tool == Some("numeric_stats"))
            .expect("expected a numeric_stats next action");
        let args = stats_action.arguments.as_ref().unwrap();
        assert_eq!(args["field"], json!("latency_ms"));
        assert_eq!(args["period"], json!("7d"));
        assert_eq!(args["app"], json!("tome"));
        assert_eq!(args["event_name"], json!("tome.search"));
    }
}
