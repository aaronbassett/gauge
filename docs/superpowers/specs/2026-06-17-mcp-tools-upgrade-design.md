# Gauge MCP Tools Upgrade — Design

- **Date:** 2026-06-17
- **Status:** Implemented (2026-06-17)
- **Scope:** `crates/gauge` (`src/mcp/`), with type touch-points in `gauge-query` and `gauge`'s `ClientError`.
- **Branch:** `mcp-tools-upgrade`

## Problem

Gauge's MCP server (`crates/gauge/src/mcp/`) exposes five tools over rmcp. The tool
descriptions and the server `instructions` are already good ("what + when", cross-references,
an explicit anonymity note). But the **response and error handling** are the two anti-patterns
that the Midnight Manual project removed in its PR #79:

1. `ok_json` (`server.rs:26`) serializes the whole query result with `serde_json::to_string_pretty`
   and drops it into a single text content block. There is no `structuredContent`, no summary
   decision line, and no follow-up guidance — the calling agent reasons over a wall of JSON.
2. `to_mcp_err` (`server.rs:21`) maps **every** `ClientError` to `McpError::internal_error` — a
   JSON-RPC error, not an `isError` tool result. Auth, bad-query, not-found, and transient cloud
   errors are indistinguishable, and the agent cannot self-correct in-conversation.
3. No tool advertises an `outputSchema`, and no tool carries MCP annotations.

This design brings gauge's MCP results in line with MCP guidance by porting the *relevant*
patterns from Midnight Manual's implementation — **excluding telemetry**, and **without** copying
the mistakes found in that implementation (empty passthrough `outputSchema`s; an inconsistent
result shape between channels).

## Goals

- A concise summary line plus a machine-readable `structuredContent` payload on every tool result.
- `suggested_next_actions` carrying concrete, ready-to-run follow-up calls built from the response.
- Tool-execution failures become `isError: true` results with a structured error envelope; JSON-RPC
  errors are reserved for protocol faults.
- A real `outputSchema` per tool that actually describes the payload.
- Standard MCP annotations on every tool.

## Non-goals

- No telemetry instrumentation of the MCP path (explicit exclusion).
- No new `status`/diagnostic tool, and no contract-snapshot test (deferred; not in this scope).
- No change to the rmcp-based architecture — gauge keeps the declarative `#[tool]` surface; input
  schemas, routing, and dispatch stay derived.
- No rewrite of the (already strong) tool descriptions or server `instructions`, beyond trimming a
  description where the same fact now lives in a schema.

## Decisions (from brainstorming)

| # | Decision | Choice |
|---|---|---|
| 1 | Stay on rmcp vs. hand-roll | Stay on rmcp 1.7; add a `render` layer that emits `CallToolResult`. |
| 2 | Result construction | Build `CallToolResult` by hand (not `CallToolResult::structured`, which dumps the raw value as text). |
| 3 | Error model | `ClientError` → closed code set, rendered as `is_error: true` results. `McpError` only for protocol faults. |
| 4 | outputSchema | Real per-tool schemas describing the payload; **not** MM's empty passthrough. |
| 5 | Annotations | `read_only_hint = true, open_world_hint = false` + `title` on all five tools, via the macro. |
| 6 | Scope | Patterns 1–5 (dual-channel, next_actions, error envelope, outputSchema, annotations). No status tool / contract test / telemetry. |

## rmcp 1.7 capabilities (verified against the vendored source)

- `CallToolResult { content: Vec<Content>, structured_content: Option<Value>, is_error: Option<bool>, meta }`
  — the struct fields are public, so the dual channel and the `isError` envelope are built directly.
  (`CallToolResult::structured(v)` exists but sets `content = [text(v.to_string())]`, which is the
  raw-dump we are removing — so we construct the struct ourselves.)
- `Tool { title, output_schema: Option<Arc<JsonObject>>, annotations: Option<ToolAnnotations>, … }`.
- `#[tool(name = "…", annotations(title = "…", read_only_hint = true, open_world_hint = false))]`
  (confirmed by `rmcp/tests/test_tool_macro_annotations.rs`).
- `output_schema()` is an overridable trait method on the tool (`tool_traits.rs:60`); the default is
  derived from the return type, so a tool returning a hand-built `CallToolResult` **must** supply
  `output_schema()` explicitly. Each tool provides its schema this way.

## The render layer (`crates/gauge/src/mcp/render.rs`, new)

Mirrors Midnight Manual's `ToolOutcome` / `ToolFailure` shape, adapted to rmcp.

```rust
pub struct NextAction {
    pub description: String,         // human sentence: what the action achieves
    pub tool: Option<&'static str>,  // None for user actions (e.g. "run `gauge login`")
    pub arguments: Option<Value>,
}

pub struct ToolOutcome {
    pub summary: String,             // concise decision line(s)
    pub trimmed: Value,              // essentials only — the fenced JSON in the text block
    pub structured: Value,           // full payload (becomes structuredContent)
    pub next_actions: Vec<NextAction>,
}
impl ToolOutcome { pub fn into_result(self) -> CallToolResult; }   // is_error: false

pub enum ErrorKind { Unauthenticated, InvalidInput, NotFound, RateLimited, CloudError, Internal }
impl ErrorKind { fn code(self) -> &'static str; fn retryable(self) -> bool; }

pub struct ToolFailure {
    pub kind: ErrorKind,
    pub message: String,             // structuredContent.error.message
    pub guidance: String,            // agent-facing recovery text in content[0]
    pub details: Value,              // merged into the error object
    pub next_actions: Vec<NextAction>,
}
impl ToolFailure { pub fn into_result(self) -> CallToolResult; }   // is_error: true
```

### Success render (`ToolOutcome::into_result`)

- `content = [ Content::text(format!("{summary}\n\n```json\n{trimmed}\n```")) ]`
- `structured_content = Some(structured)` with `suggested_next_actions` injected at the top level.
- `is_error = Some(false)`.

### Error render (`ToolFailure::into_result`)

- `content = [ Content::text(format!("{guidance}\n\n```json\n{{\"error\":{{\"code\":…,\"retryable\":…}}}}\n```")) ]`
- `structured_content = Some({ error: { code, retryable, message, ...details }, suggested_next_actions })`.
- `is_error = Some(true)`.

`server.rs` loses `ok_json` and `to_mcp_err`; each `#[tool]` fn becomes:
`self.api.query(..).await.map(project_x).unwrap_or_else(ToolFailure::from_client_error).into_result()`
(or equivalent), returning `Ok(CallToolResult)` in both success and tool-failure cases. `McpError` is
returned only for genuine protocol faults (which rmcp itself raises before our handler).

## Per-tool projections

Projectors live in `mcp::render` and take the already-deserialized response
`Value`, so they unit-test without a server. `QueryResponse` is `{ rows: Vec<Value>, truncated: bool,
elapsed_ms: u64 }`.

| Tool | Summary line | Trimmed fence | suggested_next_actions |
|---|---|---|---|
| `get_meta` | `"Apps: tome, midnight-manual. 14 event names, 1.2M events over 34d."` | apps + counts | per top app: `top_events {period:"30d", app}`; one ready `query_telemetry` example with a real app + event |
| `query_telemetry` | `"3 rows, 12ms (truncated: false)."` | first N rows | if `granularity` unset → `events_over_time` for the same filter; if dimensioned → narrow to the top row's dimension value |
| `unique_users` | `"412 unique installs (tome, 7d)."` | the count + filters | `top_events {same period, app}` ("what are those users doing") |
| `top_events` | `"Top 3 (30d): tome.search 1,204 · tome.open 980 · tome.exit 654."` | ranked list | for the top event: `unique_users {event_name}` and `events_over_time {event_name}` |
| `events_over_time` | `"7 day-buckets (7d), peak 312 on 2026-06-14."` | bucket summary | `top_events {same window}` (what's driving the volume) |

Rules:
- Summaries are a single decision line wherever possible; the agent can decide to drill in without
  reading any JSON.
- The trimmed fence stays lean (no full row payloads when the row set is large); the full set is
  always in `structuredContent`. For small results (e.g. a single scalar `unique_users`) the trimmed
  fence may equal the structured payload — accepted, mirrors MM's single-chunk case.
- Every `arguments` value uses **real** values pulled from the current response (real app names, the
  top event name), never placeholders.

## Error mapping (`ToolFailure::from_client_error`)

| `ClientError` variant | `ErrorKind` | code | retryable | next_action |
|---|---|---|---|---|
| `Auth` / `KeyMissing` / `KeyExists` / `ConfigMissing` / `ConfigInvalid` / `NoConfigDir` | `Unauthenticated` | `UNAUTHENTICATED` | false | user: "run `gauge login`" (or the variant's existing remediation) |
| `Api { status: 400, .. }` and pre-flight validation failures | `InvalidInput` | `INVALID_INPUT` | true | guidance names the field; `get_meta` to discover valid apps/events |
| `Api { status: 404, .. }` | `NotFound` | `NOT_FOUND` | false | `get_meta` |
| `Api { status: 401 \| 403, .. }` | `Unauthenticated` | `UNAUTHENTICATED` | false | user: re-auth |
| `Api { status: 429, .. }` | `RateLimited` | `RATE_LIMITED` | true | retry after the limit resets |
| `Api { status: 5xx, .. }` / `Http` | `CloudError` | `CLOUD_ERROR` | true | transient — retry; `get_meta`/server may be down |
| `Io` / `Json` | `Internal` | `INTERNAL` | false | — |

- `ClientError::Api` already carries `code`, `message`, and `remediation`; these flow into the
  envelope (`message` ← `message`, `details.server_code` ← `code`, `guidance` ← `remediation` when set).
- `query_telemetry` runs `gauge_query::validate(&req)` **before** the network call, so an invalid DSL
  request returns `INVALID_INPUT` naming the offending field rather than an opaque error.
- `retryable: false` means an identical retry cannot succeed; recovery rides on `suggested_next_actions`
  / `guidance` (same semantics as MM).

## outputSchema (real, not passthrough)

A shared `mcp::schemas` module, one `fn <tool>_output_schema() -> Value` per tool, each supplied to
rmcp via the tool's `output_schema()` override.

- **Query tools** (`query_telemetry`, `unique_users`, `top_events`, `events_over_time`) share a
  gauge-owned envelope:
  ```jsonc
  {
    "type": "object",
    "required": ["rows", "truncated", "elapsed_ms"],
    "properties": {
      "rows": { "type": "array", "items": { "type": "object", "additionalProperties": true },
                "description": "One object per row, keyed by output aliases (measure names, dimension strings, \"time_bucket\")." },
      "truncated": { "type": "boolean", "description": "True if the row cap was hit and rows were dropped." },
      "elapsed_ms": { "type": "integer" },
      "suggested_next_actions": { /* shared fragment: {description, tool?, arguments?} */ }
    },
    "additionalProperties": true
  }
  ```
  Per-row keys are inherently dynamic (DSL aliases), so `rows.items` stays open — but this is
  **documented in the schema**, and the envelope itself is fully pinned. This is the explicit
  contrast with MM's empty passthrough: the wrapper, the booleans, and the action shape are described.
- **`get_meta`** gets its own schema describing `apps[]`, event names, attribute keys, totals, and the
  time span, matching the `meta()` response type.
- The `suggested_next_actions` fragment is shared (`{ description (required), tool?, arguments? }`).

## Annotations

Via the macro on all five tools: `annotations(title = "…", read_only_hint = true, open_world_hint = false)`.
All five are read-only queries over a closed database, so the hints are uniform; `title` is a short
human label (e.g. "Query telemetry", "Discover schema", "Unique users", "Top events", "Events over time").

## Testing

Extend `crates/gauge/src/mcp/tools.rs` unit tests (projectors take `Value`, so no live server):

- **Success shape:** each projector yields a non-empty `summary`, `structured_content` present with
  `suggested_next_actions`, and `is_error == Some(false)`.
- **Trimmed view:** the fenced JSON is a strict subset of `structured` (no large row payloads in the
  fence for multi-row results).
- **Next-action grounding:** `arguments` contain real values from the input response (e.g. `top_events`
  on a fixture surfaces the fixture's top event id in its `unique_users` action).
- **Error mapping:** a table test maps each `ClientError` variant to the expected `code` / `retryable`,
  and asserts the result is `is_error: true` (not an `McpError`).
- **outputSchema fidelity:** the query envelope schema lists `rows` / `truncated` / `elapsed_ms`
  (a real assertion, unlike MM's near-vacuous conformance check); `get_meta`'s schema lists its
  documented fields. Existing `tool_param_schemas_generate_and_describe_fields` test stays.

## Files touched

- `crates/gauge/src/mcp/render.rs` — **new**: `NextAction`, `ToolOutcome`, `ToolFailure`, `ErrorKind`,
  per-tool projectors, `from_client_error`.
- `crates/gauge/src/mcp/schemas.rs` — **new**: per-tool `output_schema()` bodies + shared fragments.
- `crates/gauge/src/mcp/server.rs` — delete `ok_json` / `to_mcp_err`; tool fns become adapters
  returning hand-built `CallToolResult`; add `annotations(...)` + `output_schema()` per tool.
- `crates/gauge/src/mcp/mod.rs` — register the new modules.
- `crates/gauge/src/mcp/tools.rs` — projector unit tests (and any param-struct touch-ups).
- `crates/gauge/src/error.rs` — no behavior change expected; a `status_class()` helper on `Api` may be
  added to keep the mapping table tidy.

## Risks / trade-offs

- **Manual `CallToolResult` construction** couples gauge to rmcp's result struct shape (vs. the derive).
  Accepted: it is the only way to get a custom summary + trimmed fence + structured payload together.
- **Redundant presentation** for small results (single-scalar tools repeat the value in both channels).
  Accepted and bounded — mirrors MM's documented single-chunk case.
- **Open `rows.items`** means the per-row contract isn't machine-validated. Inherent to the DSL; the
  envelope is pinned and the openness is documented, which is the most that's honest here.

## Out of scope / deferred

- Telemetry on the MCP path.
- A `status`/diagnostic tool and a contract-snapshot test (both viable future follow-ups).
- Touching the non-MCP query API, the DSL, or auth.
