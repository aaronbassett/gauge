# Gauge MCP Tools Upgrade Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring gauge's MCP tool results up to standard — dual-channel responses (summary + `structuredContent`), `suggested_next_actions`, an `isError` error envelope, real per-tool `outputSchema`, and tool annotations — while staying on the rmcp declarative surface.

**Architecture:** Add a `render` module (`ToolOutcome`/`ToolFailure` → hand-built `CallToolResult`) and a `schemas` module (per-tool `outputSchema` injected via a manual `ServerHandler::list_tools`). Each `#[tool]` fn becomes a thin adapter that projects the cloud response into a `ToolOutcome` or maps a `ClientError` into a `ToolFailure`. `McpError` is reserved for protocol faults.

**Tech Stack:** Rust 2024, `rmcp` 1.7 (`server`, `transport-io`), `serde_json`, `schemars`, the in-repo `gauge_query` DSL.

**Spec:** `docs/superpowers/specs/2026-06-17-mcp-tools-upgrade-design.md`

---

## File Structure

- `crates/gauge/src/mcp/render.rs` — **new.** `NextAction`, `ToolOutcome`, `ErrorKind`, `ToolFailure`, `ToolFailure::from_client_error`, and the five projectors. ~280 lines.
- `crates/gauge/src/mcp/schemas.rs` — **new.** Hand-authored `outputSchema` per tool + `apply_output_schemas(&mut [Tool])`. ~110 lines.
- `crates/gauge/src/mcp/mod.rs` — **modify.** Register the two new modules.
- `crates/gauge/src/mcp/server.rs` — **modify.** Delete `ok_json`/`to_mcp_err`; tool fns become adapters; add annotations; hand-write `ServerHandler` (`get_info` + `list_tools` schema injection + `call_tool` delegation).
- `crates/gauge/src/mcp/tools.rs` — unchanged except its existing tests stay green (param→query builders are reused by the projectors' callers).

### Key types (defined in Task 1–2, referenced everywhere after)

```rust
pub struct NextAction { pub description: String, pub tool: Option<&'static str>, pub arguments: Option<Value> }
pub struct ToolOutcome { pub summary: String, pub trimmed: Value, pub structured: Value, pub next_actions: Vec<NextAction> }
pub enum ErrorKind { Unauthenticated, InvalidInput, NotFound, RateLimited, CloudError, Internal }
pub struct ToolFailure { pub kind: ErrorKind, pub message: String, pub guidance: String, pub details: Value, pub next_actions: Vec<NextAction> }
```

Projector signatures (all take the response as `&Value`, so they unit-test without a server):

```rust
fn project_query(resp: &Value, req: &QueryRequest) -> ToolOutcome
fn project_meta(resp: &Value) -> ToolOutcome
fn project_unique_users(resp: &Value, p: &UniqueUsersParams) -> ToolOutcome
fn project_top_events(resp: &Value, p: &TopEventsParams) -> ToolOutcome
fn project_events_over_time(resp: &Value, p: &EventsOverTimeParams) -> ToolOutcome
```

---

## Task 1: Render module scaffold — `NextAction` + `ToolOutcome`

**Files:**
- Create: `crates/gauge/src/mcp/render.rs`
- Modify: `crates/gauge/src/mcp/mod.rs`

- [ ] **Step 1: Register the module**

Edit `crates/gauge/src/mcp/mod.rs` to add the new module (keep the existing lines):

```rust
pub mod render;
pub mod schemas;
pub mod server;
pub mod tools;
```

(If `schemas` does not exist yet the crate won't build — that's fine, the next step creates `render` and Task 3 creates `schemas`; do Step 2 before building.)

- [ ] **Step 2: Write `render.rs` with `NextAction` + `ToolOutcome` + a failing test**

Create `crates/gauge/src/mcp/render.rs`:

```rust
//! Shapes every MCP tool result into the "summary + structuredContent" form.
//!
//! Success → one text block (summary + trimmed JSON fence) plus full-fidelity
//! `structured_content` (with `suggested_next_actions`). Failure → an
//! `is_error: true` result carrying a shared error envelope.

use rmcp::model::{CallToolResult, Content};
use serde_json::{json, Value};

/// A suggested follow-up. `tool: None` describes a user action (e.g. "run `gauge login`").
#[derive(Debug, Clone)]
pub struct NextAction {
    pub description: String,
    pub tool: Option<&'static str>,
    pub arguments: Option<Value>,
}

impl NextAction {
    pub fn call(description: impl Into<String>, tool: &'static str, arguments: Value) -> Self {
        Self { description: description.into(), tool: Some(tool), arguments: Some(arguments) }
    }
    pub fn user(description: impl Into<String>) -> Self {
        Self { description: description.into(), tool: None, arguments: None }
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
            map.insert("suggested_next_actions".to_owned(), actions_value(&self.next_actions));
        }
        let trimmed = serde_json::to_string(&self.trimmed).unwrap_or_else(|_| "{}".to_owned());
        let text = format!("{}\n\n```json\n{trimmed}\n```", self.summary);
        CallToolResult {
            content: vec![Content::text(text)],
            structured_content: Some(structured),
            is_error: Some(false),
            meta: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let text = result.content[0].as_text().expect("text content").text.clone();
        assert!(text.starts_with("2 rows, 9ms."));
        assert!(text.contains("```json"));
        // structured_content carries the payload + injected suggested_next_actions
        let sc = result.structured_content.expect("structured_content present");
        assert_eq!(sc["truncated"], json!(false));
        assert_eq!(sc["suggested_next_actions"][0]["tool"], json!("get_meta"));
    }
}
```

> `Content` is `Annotated<RawContent>`; `as_text()` (verified present in rmcp 1.7, `model/content.rs:211`) is the stable accessor. Production code never inspects content this way — only the test does.

- [ ] **Step 3: Build only `render` to verify it compiles (schemas not yet present)**

Temporarily comment out `pub mod schemas;` in `mod.rs`, then:

Run: `cargo test -p gauge render::tests::outcome_renders_summary_fence_and_structured`
Expected: PASS. Then restore `pub mod schemas;` (it will fail to build until Task 3 — that's expected; the next tasks add `render` content first, then `schemas`).

> To keep the tree buildable between tasks, leave `pub mod schemas;` commented out until Task 3 Step 1, where you create the file and uncomment it.

- [ ] **Step 4: Commit**

```bash
git add crates/gauge/src/mcp/render.rs crates/gauge/src/mcp/mod.rs
git commit -m "feat(mcp): render scaffold — NextAction + ToolOutcome"
```

---

## Task 2: Error envelope — `ErrorKind` + `ToolFailure` + `from_client_error`

**Files:**
- Modify: `crates/gauge/src/mcp/render.rs`

- [ ] **Step 1: Append the failing test**

Add to the `#[cfg(test)] mod tests` block in `render.rs`:

```rust
    use crate::error::ClientError;

    #[test]
    fn client_errors_map_to_codes() {
        let cases = [
            (ClientError::NoConfigDir, "UNAUTHENTICATED", false),
            (ClientError::Http("boom".into()), "CLOUD_ERROR", true),
            (
                ClientError::Api { status: 400, code: "bad".into(), message: "nope".into(), remediation: None },
                "INVALID_INPUT",
                true,
            ),
            (
                ClientError::Api { status: 404, code: "missing".into(), message: "nope".into(), remediation: None },
                "NOT_FOUND",
                false,
            ),
            (
                ClientError::Api { status: 429, code: "slow".into(), message: "nope".into(), remediation: None },
                "RATE_LIMITED",
                true,
            ),
            (
                ClientError::Api { status: 503, code: "down".into(), message: "nope".into(), remediation: None },
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p gauge render::tests::client_errors_map_to_codes`
Expected: FAIL (`ToolFailure` / `ErrorKind` not found).

- [ ] **Step 3: Implement the envelope**

Add to `render.rs` (above the `#[cfg(test)]` block):

```rust
use crate::error::ClientError;

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
        matches!(self, ErrorKind::InvalidInput | ErrorKind::RateLimited | ErrorKind::CloudError)
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
            ClientError::Api { status, code, message, remediation } => {
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
        let structured = json!({ "error": error, "suggested_next_actions": actions_value(&self.next_actions) });
        let trimmed = serde_json::to_string(
            &json!({ "error": { "code": self.kind.code(), "retryable": self.kind.retryable() } }),
        )
        .unwrap_or_else(|_| "{}".to_owned());
        let text = format!("{}\n\n```json\n{trimmed}\n```", self.guidance);
        CallToolResult {
            content: vec![Content::text(text)],
            structured_content: Some(structured),
            is_error: Some(true),
            meta: None,
        }
    }
}
```

> The leading `let msg = e.to_string();` is deliberate: the `Api` arm moves `message`/`code`/`remediation` out of `e`, so the message for the other arms must be captured before the `match` consumes `e`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p gauge render::tests::client_errors_map_to_codes`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/gauge/src/mcp/render.rs
git commit -m "feat(mcp): isError error envelope (ErrorKind + ToolFailure)"
```

---

## Task 3: Output schemas module

**Files:**
- Create: `crates/gauge/src/mcp/schemas.rs`
- Modify: `crates/gauge/src/mcp/mod.rs` (uncomment `pub mod schemas;` if it was commented in Task 1)

- [ ] **Step 1: Create `schemas.rs` with a failing test**

Create `crates/gauge/src/mcp/schemas.rs`:

```rust
//! Hand-authored `outputSchema` per tool, injected onto router-built tools by
//! name in `ServerHandler::list_tools`. Unlike a passthrough schema, each one
//! describes the actual payload envelope.

use std::sync::Arc;

use rmcp::model::Tool;
use serde_json::{json, Value};

fn next_actions_fragment() -> Value {
    json!({
        "type": "array",
        "items": {
            "type": "object",
            "required": ["description"],
            "properties": {
                "description": { "type": "string", "description": "What this suggested action achieves." },
                "tool": { "type": "string", "description": "Tool to call. Absent for user actions." },
                "arguments": { "type": "object" }
            }
        }
    })
}

/// Shared envelope for the four query-returning tools.
fn query_envelope_schema() -> Value {
    json!({
        "type": "object",
        "required": ["rows", "truncated", "elapsed_ms"],
        "additionalProperties": true,
        "properties": {
            "rows": {
                "type": "array",
                "items": { "type": "object", "additionalProperties": true },
                "description": "One object per row, keyed by output aliases (measure names, dimension strings, \"time_bucket\")."
            },
            "truncated": { "type": "boolean", "description": "True if the row cap was hit and rows were dropped." },
            "elapsed_ms": { "type": "integer" },
            "suggested_next_actions": next_actions_fragment()
        }
    })
}

fn meta_schema() -> Value {
    json!({
        "type": "object",
        "required": ["apps"],
        "additionalProperties": true,
        "properties": {
            "apps": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["app", "event_names", "attribute_keys", "total_events"],
                    "properties": {
                        "app": { "type": "string" },
                        "event_names": { "type": "array", "items": { "type": "string" } },
                        "attribute_keys": { "type": "array", "items": { "type": "string" } },
                        "first_event": { "type": "string", "description": "RFC3339; absent when the app has no events." },
                        "last_event": { "type": "string", "description": "RFC3339; absent when the app has no events." },
                        "total_events": { "type": "integer" }
                    }
                }
            },
            "suggested_next_actions": next_actions_fragment()
        }
    })
}

fn schema_for(name: &str) -> Option<Value> {
    match name {
        "get_meta" => Some(meta_schema()),
        "query_telemetry" | "unique_users" | "top_events" | "events_over_time" => {
            Some(query_envelope_schema())
        }
        _ => None,
    }
}

/// Patch hand-authored output schemas onto router-built tools, matched by name.
pub fn apply_output_schemas(tools: &mut [Tool]) {
    for t in tools.iter_mut() {
        if let Some(Value::Object(map)) = schema_for(&t.name) {
            t.output_schema = Some(Arc::new(map));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::Tool;
    use std::sync::Arc;

    #[test]
    fn query_envelope_describes_real_fields() {
        let s = query_envelope_schema();
        let props = s["properties"].as_object().unwrap();
        for key in ["rows", "truncated", "elapsed_ms", "suggested_next_actions"] {
            assert!(props.contains_key(key), "query envelope missing `{key}`");
        }
        assert_eq!(s["required"], json!(["rows", "truncated", "elapsed_ms"]));
    }

    #[test]
    fn meta_schema_lists_app_fields() {
        let s = meta_schema();
        let app_props = s["properties"]["apps"]["items"]["properties"].as_object().unwrap();
        for key in ["app", "event_names", "attribute_keys", "total_events"] {
            assert!(app_props.contains_key(key), "meta app schema missing `{key}`");
        }
    }

    #[test]
    fn apply_sets_schema_for_known_tools_only() {
        let empty = Arc::new(serde_json::Map::new());
        let mut tools = vec![
            Tool::new("query_telemetry", "q", empty.clone()),
            Tool::new("get_meta", "m", empty.clone()),
            Tool::new("some_unknown_tool", "u", empty.clone()),
        ];
        apply_output_schemas(&mut tools);
        assert!(tools[0].output_schema.is_some());
        assert!(tools[1].output_schema.is_some());
        assert!(tools[2].output_schema.is_none());
    }
}
```

Ensure `crates/gauge/src/mcp/mod.rs` has `pub mod schemas;` (uncomment if needed).

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test -p gauge schemas::`
Expected: PASS (3 tests).

> If `Tool::new`'s third argument type differs (it expects `Arc<JsonObject>` = `Arc<serde_json::Map<String, Value>>`), the `empty` value above already matches; no change needed.

- [ ] **Step 3: Commit**

```bash
git add crates/gauge/src/mcp/schemas.rs crates/gauge/src/mcp/mod.rs
git commit -m "feat(mcp): real per-tool outputSchema + apply_output_schemas"
```

---

## Task 4: Projectors — `project_meta` + `project_query`

**Files:**
- Modify: `crates/gauge/src/mcp/render.rs`

- [ ] **Step 1: Add failing tests**

Add to `render.rs` `#[cfg(test)] mod tests`:

```rust
    use crate::mcp::tools::{EventsOverTimeParams, TopEventsParams, TopBy, UniqueUsersParams};
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
        let drill = o.next_actions.iter().find(|a| a.tool == Some("top_events")).unwrap();
        assert_eq!(drill.arguments.as_ref().unwrap()["app"], json!("tome"));
    }

    #[test]
    fn project_query_reports_rowcount_and_trend_action() {
        let resp = json!({ "rows": [ {"count": 5}, {"count": 3} ], "truncated": false, "elapsed_ms": 11 });
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
        let trend = o.next_actions.iter().find(|a| a.tool == Some("events_over_time")).unwrap();
        assert_eq!(trend.arguments.as_ref().unwrap()["period"], json!("7d"));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p gauge render::tests::project_meta_summarizes_and_suggests_drill`
Expected: FAIL (`project_meta` not found).

- [ ] **Step 3: Implement `project_meta` + `project_query`**

Add to `render.rs` (above the test module). Add `use gauge_query::{QueryRequest, TimeRange};` to the file's imports.

```rust
use gauge_query::{QueryRequest, TimeRange};

/// Up to this many rows go in the text fence; the full set is always in structured_content.
const FENCE_ROW_CAP: usize = 3;

fn rows_of(resp: &Value) -> &[Value] {
    resp.get("rows").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

/// `get_meta` → MetaResponse `{ apps: [AppMeta..] }`.
pub fn project_meta(resp: &Value) -> ToolOutcome {
    let empty = vec![];
    let apps = resp.get("apps").and_then(Value::as_array).unwrap_or(&empty);
    let names: Vec<&str> = apps.iter().filter_map(|a| a.get("app").and_then(Value::as_str)).collect();
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
        if names.is_empty() { "(none)".to_owned() } else { names.join(", ") },
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
    ToolOutcome { summary, trimmed, structured: resp.clone(), next_actions }
}

/// Generic query result `{ rows, truncated, elapsed_ms }` for `query_telemetry`.
pub fn project_query(resp: &Value, req: &QueryRequest) -> ToolOutcome {
    let rows = rows_of(resp);
    let truncated = resp.get("truncated").and_then(Value::as_bool).unwrap_or(false);
    let elapsed = resp.get("elapsed_ms").and_then(Value::as_u64).unwrap_or(0);
    let summary = format!("{} rows, {elapsed}ms (truncated: {truncated}).", rows.len());
    let mut next_actions = vec![NextAction::call(
        "Discover queryable apps, event names, and attribute keys",
        "get_meta",
        json!({}),
    )];
    // No granularity + a relative range → offer a day-bucketed trend over the same period.
    if req.granularity.is_none() {
        if let TimeRange::Last { last } = &req.time_range {
            next_actions.push(NextAction::call(
                "View this as a day-by-day trend over the same period",
                "events_over_time",
                json!({ "period": last, "granularity": "day" }),
            ));
        }
    }
    let trimmed = json!({
        "rows": rows.iter().take(FENCE_ROW_CAP).cloned().collect::<Vec<_>>(),
        "row_count": rows.len(),
        "truncated": truncated,
    });
    ToolOutcome { summary, trimmed, structured: resp.clone(), next_actions }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p gauge render::tests::project_meta_summarizes_and_suggests_drill render::tests::project_query_reports_rowcount_and_trend_action`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/gauge/src/mcp/render.rs
git commit -m "feat(mcp): project_meta + project_query projectors"
```

---

## Task 5: Projectors — `project_unique_users` + `project_top_events` + `project_events_over_time`

**Files:**
- Modify: `crates/gauge/src/mcp/render.rs`

- [ ] **Step 1: Add failing tests**

Add to `render.rs` test module:

```rust
    #[test]
    fn project_top_events_ranks_and_drills_top() {
        let resp = json!({
            "rows": [
                { "event_name": "tome.search", "count": 1204 },
                { "event_name": "tome.open", "count": 980 }
            ],
            "truncated": false, "elapsed_ms": 8
        });
        let p = TopEventsParams { period: "30d".into(), app: Some("tome".into()), by: None, limit: None };
        let o = project_top_events(&resp, &p);
        assert!(o.summary.contains("tome.search"));
        // drill the top event into unique_users
        let drill = o.next_actions.iter().find(|a| a.tool == Some("unique_users")).unwrap();
        assert_eq!(drill.arguments.as_ref().unwrap()["event_name"], json!("tome.search"));
    }

    #[test]
    fn project_unique_users_reads_scalar() {
        let resp = json!({ "rows": [ { "unique_installs": 412 } ], "truncated": false, "elapsed_ms": 6 });
        let p = UniqueUsersParams { period: "7d".into(), app: Some("tome".into()), event_name: None };
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
        let p = EventsOverTimeParams { period: "7d".into(), granularity: gauge_query::Granularity::Day, app: None, event_name: None };
        let o = project_events_over_time(&resp, &p);
        assert!(o.summary.contains("312"));
        assert!(o.summary.contains("2026-06-14"));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p gauge render::tests::project_top_events_ranks_and_drills_top`
Expected: FAIL (`project_top_events` not found).

- [ ] **Step 3: Implement the three projectors**

Add to `render.rs`. Add `use crate::mcp::tools::{EventsOverTimeParams, TopBy, TopEventsParams, UniqueUsersParams};` to the file imports.

```rust
use crate::mcp::tools::{EventsOverTimeParams, TopBy, TopEventsParams, UniqueUsersParams};

/// `unique_users` → single-row `{ unique_installs: N }`.
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
    ToolOutcome { summary, trimmed, structured: resp.clone(), next_actions }
}

/// `top_events` → rows `{ event_name, <measure> }` ranked desc.
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
    let summary = format!("Top {} ({}): {}.", rows.len().min(3), p.period, head.join(" · "));
    let mut next_actions = Vec::new();
    if let Some(top) = rows.first().and_then(|r| r.get("event_name")).and_then(Value::as_str) {
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
    ToolOutcome { summary, trimmed, structured: resp.clone(), next_actions }
}

/// `events_over_time` → rows `{ time_bucket, count }`.
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
    let mut next_actions = Vec::new();
    if let Some(app) = &p.app {
        next_actions.push(NextAction::call(
            "Break the same window down by event type",
            "top_events",
            json!({ "period": p.period, "app": app }),
        ));
    } else {
        next_actions.push(NextAction::call(
            "Break the same window down by event type",
            "top_events",
            json!({ "period": p.period }),
        ));
    }
    let trimmed = json!({ "buckets": rows.len(), "rows": rows.iter().take(FENCE_ROW_CAP).cloned().collect::<Vec<_>>() });
    ToolOutcome { summary, trimmed, structured: resp.clone(), next_actions }
}
```

> `TopBy` must derive `Clone, Copy` for `p.by.unwrap_or(TopBy::Count)` to copy out of `&p`. It already does (`#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]` in `tools.rs`). `UniqueUsersParams`/`TopEventsParams`/`EventsOverTimeParams` fields are `pub`, so constructing them in tests works.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p gauge render::`
Expected: PASS (all render tests).

- [ ] **Step 5: Commit**

```bash
git add crates/gauge/src/mcp/render.rs
git commit -m "feat(mcp): unique_users + top_events + events_over_time projectors"
```

---

## Task 6: Wire `server.rs` — adapters, annotations, manual `ServerHandler`

**Files:**
- Modify: `crates/gauge/src/mcp/server.rs`

- [ ] **Step 1: Rewrite the imports + helpers**

Replace the top of `server.rs` (imports through `ok_json`) with:

```rust
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
use serde_json::{json, Value};

use crate::api::ApiClient;
use crate::error::ClientError;
use crate::mcp::render::{
    project_events_over_time, project_meta, project_query, project_top_events,
    project_unique_users, ErrorKind, NextAction, ToolFailure,
};
use crate::mcp::schemas::apply_output_schemas;
use crate::mcp::tools::{
    events_over_time_query, top_events_query, unique_users_query, EventsOverTimeParams,
    TopEventsParams, UniqueUsersParams,
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
```

> Removed: `to_mcp_err` and `ok_json` (deleted entirely). `Content` import is dropped from `server.rs` (it now lives in `render.rs`).

- [ ] **Step 2: Replace the `#[tool_router] impl GaugeMcp` block**

Replace the whole `#[tool_router] impl GaugeMcp { … }` block with the following. Each tool keeps its description as a doc comment (the macro extracts the description from doc comments) and gains annotations:

```rust
#[tool_router]
impl GaugeMcp {
    pub fn new(api: Arc<ApiClient>) -> Self {
        Self { api, tool_router: Self::tool_router() }
    }

    /// Run an analytics query over anonymous telemetry events. Measures: count, unique_installs, unique_sessions. Dimensions: app, event_name, app_version, os, arch, attr.<key>. Time ranges: {"last":"7d"} or RFC3339 from/to. Use get_meta first to discover apps, event names, and attribute keys.
    #[tool(annotations(title = "Query telemetry", read_only_hint = true, open_world_hint = false))]
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
    #[tool(annotations(title = "Discover schema", read_only_hint = true, open_world_hint = false))]
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
    #[tool(annotations(title = "Events over time", read_only_hint = true, open_world_hint = false))]
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
```

- [ ] **Step 3: Replace `#[tool_handler] impl ServerHandler` with a manual impl**

Replace the `#[tool_handler] impl ServerHandler for GaugeMcp { … }` block with:

```rust
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
        Ok(ListToolsResult { tools, ..Default::default() })
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
```

Leave `pub async fn serve(...)` at the bottom unchanged.

- [ ] **Step 4: Build the crate**

Run: `cargo build -p gauge`
Expected: SUCCESS.

Likely fix-ups if it does not compile (adjust imports only, not logic):
- `RoleServer` path: try `rmcp::RoleServer` if `rmcp::service::RoleServer` is not found.
- `ToolCallContext` path: confirm with `grep -rn "pub struct ToolCallContext" ~/.cargo/registry/src/*/rmcp-1.7.0/src`.
- If `async fn` in the `ServerHandler` impl is rejected, the trait methods accept `async fn` in rmcp 1.7 (the `#[tool_handler]` macro emits the same); ensure the crate has no `#![deny(...)]` blocking it.

- [ ] **Step 5: Run the full crate test suite**

Run: `cargo test -p gauge`
Expected: PASS (existing `mcp::tools` builder tests + all new `render::`/`schemas::` tests).

- [ ] **Step 6: Commit**

```bash
git add crates/gauge/src/mcp/server.rs
git commit -m "feat(mcp): dual-channel results, isError envelope, annotations, output schemas"
```

---

## Task 7: Verification + spec status

**Files:**
- Modify: `docs/superpowers/specs/2026-06-17-mcp-tools-upgrade-design.md`

- [ ] **Step 1: Format + lint + test (workspace)**

Run: `cargo fmt --all`
Run: `cargo clippy -p gauge --all-targets -- -D warnings`
Expected: clean (no warnings).
Run: `cargo test -p gauge`
Expected: PASS.

- [ ] **Step 2: Manual smoke check of the tool surface (optional but recommended)**

Run a quick listing through the server if a local config exists:

Run: `printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"x","version":"0"}}}' '{"jsonrpc":"2.0","method":"notifications/initialized"}' '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' | cargo run -p gauge -- mcp serve 2>/dev/null`
Expected: the `tools/list` response shows each tool with `annotations` (`readOnlyHint`, `title`) and an `outputSchema` whose `properties` include `rows`/`truncated`/`elapsed_ms` (query tools) or `apps` (get_meta).

> If `mnm mcp serve`'s exact subcommand differs, check `cargo run -p gauge -- --help`. Skip this step if no local gauge config/auth is present; the unit tests already cover shape.

- [ ] **Step 3: Mark the spec implemented**

In `docs/superpowers/specs/2026-06-17-mcp-tools-upgrade-design.md`, change the header `**Status:** Approved (brainstorming)` to `**Status:** Implemented (2026-06-17)`.

- [ ] **Step 4: Final commit**

```bash
git add docs/superpowers/specs/2026-06-17-mcp-tools-upgrade-design.md
git commit -m "docs(spec): mark MCP tools upgrade implemented"
```

---

## Self-Review Notes (for the executor)

- **Spec coverage:** dual-channel (Task 1), suggested_next_actions (Tasks 4–5 projectors), isError envelope (Task 2), real outputSchema (Task 3 + injection in Task 6), annotations (Task 6 Step 2). Error mapping table → `from_client_error` (Task 2). Out-of-scope items (telemetry, status tool, contract test) are not implemented, as intended.
- **Type consistency:** projector names are identical in their definitions (Tasks 4–5) and their `use`/call sites (Task 6). `ErrorKind` variants match between Task 2's definition and the `from_client_error` arms. `apply_output_schemas` is defined in Task 3 and called in Task 6.
- **No live-server dependency in tests:** every projector and the schema injection are tested over plain `Value` / hand-built `Tool`s, so the suite runs without config or network.
