# Read-time numeric bucketing, aggregates & filters in the query DSL — Design

- **Date:** 2026-06-17
- **Status:** Approved design; ready for implementation planning.
- **Issue:** [aaronbassett/gauge#22](https://github.com/aaronbassett/gauge/issues/22)
- **Companion:** [`gauge-telemetry` kernel design](2026-06-17-gauge-telemetry-kernel-design.md)
  (the kernel ships quantities as **raw bounded integers** and defers all bucketing
  to read time — this design is the read-time half that makes that data useful).

---

## 1. Context & problem

Gauge's query DSL (`gauge-query`) can group/filter on **exact text** values and
bucket by **time** (`date_trunc`), but it has **no support for numeric
attributes**. Numeric `attr.*` values (e.g. `latency_ms`, `duration_ms`,
`cpu_cores`, `ram_gb`, counts, rank) are stored in JSONB but are only addressable
**as text** — so a raw `attr.latency_ms` can be grouped by exact value or
`=`/`in`-filtered as a string, but it **cannot be histogrammed, averaged, or
range-filtered**.

The telemetry kernel deliberately sends raw integers so bucket edges are never
frozen into shipped binaries and the data stays re-aggregatable. For that decision
to pay off, the **server must bucket/aggregate those raw integers at query time**,
and — because the DSL is shared — the change must surface coherently across **every
consumer**: the server SQL builder, the CLI one-shot, the MCP agent surface, and
the TUI dashboard.

### Current shape (as of `9535951`)

- `Measure` = `count | unique_installs | unique_sessions` — no `avg/min/max/percentile`.
- `FilterOp` = `eq | neq | in | exists` — all compare the JSONB attribute as text; no `gt/gte/lt/lte`.
- `dimensions: Vec<Field>` — group by exact value; the only bucketing is `time_bucket` via `date_trunc`.
- `QueryResponse` = `{ rows, truncated, elapsed_ms }` — no response-level metadata.
- Consumers: server `sqlbuild.rs` → `routes/query.rs` (decode by `ColKind`); CLI
  `query_cmd.rs` (raw passthrough); MCP `mcp/{server,tools,render,schemas}.rs`; TUI
  `tui/{app,data,ui}.rs` (fixed query shapes + a hardcoded Explore picker).

## 2. Goals / non-goals

**Goals**
- First-class **numeric bucket dimension** with caller-supplied edges (read time).
- Numeric **aggregate measures**: `avg`, `min`, `max`, `p50`, `p90`, `p95`, `p99`.
- Numeric **comparison filters**: `gt`, `gte`, `lt`, `lte`.
- One shared JSONB→numeric cast with graceful null handling (non-numeric/null rows
  are **excluded**, never error).
- The capability surfaces, end to end, across **CLI, MCP, and TUI**, not just the server.
- Preserve all existing safety invariants (closed-enum identifiers, every value
  bound, `install_id`/`session_id` non-addressable).

**Non-goals**
- Arbitrary SQL / computed expressions beyond the closed aggregate set.
- Numeric ops on envelope columns (`app`, `os`, `arch`, …) — numeric ops are
  **`attr.*`-only** (envelope columns are text).
- Changing the ingest/storage format (kernel already sends JSON numbers).

## 3. Decisions (locked, from brainstorming)

| # | Decision | Rationale |
|---|---|---|
| D1 | **Hybrid bucket wire shape**: the row carries the human-readable range **label**; `QueryResponse` gains an optional `meta.buckets` echoing **edges + full label set**; SQL groups/orders by the `width_bucket` **integer index**, decode maps index→label. | Self-describing rows for the CLI/agents, correct numeric ordering for free, and richer clients (MCP labels, TUI histogram) get the full bucket set including empty buckets. Index-in-SQL avoids lexical mis-ordering of label strings. |
| D2 | **Percentiles included now** (`p50/p90/p95/p99` via `percentile_cont`) alongside `avg/min/max`. | Percentiles over raw latency/duration are the kernel's headline payoff; modest extra SQL and one float decode path that `avg` needs anyway. |
| D3 | **Full cross-project scope**: server + CLI docs + MCP (description, render, **two** convenience tools) + TUI numeric view. | The issue's value is only realized when agents and humans can actually ask numeric questions; the DSL is shared, so drift between surfaces is the main risk. |
| D4 | **Two MCP convenience tools**: `numeric_stats` and `numeric_histogram`, cross-linked via `suggested_next_actions`. | Agents answer reliably from purpose-named tools; histogram edge-syntax and percentile composition are exactly what they fumble in the raw DSL. |
| D5 | Add **`AppMeta.numeric_attribute_keys`** (server detects `jsonb_typeof='number'`). | Lets `get_meta` and the TUI picker offer **valid** numeric attrs instead of guessing; small additive change that improves the whole numeric surface. |
| D6 | **TUI histogram edges derived from a min/max probe** (~6 nicely-rounded even edges across the observed range). | Auto-fit histograms beat fixed edges for a dashboard; edges remain fully caller-controlled everywhere else (DSL/MCP/CLI). |
| D7 | One shared safe cast: `CASE WHEN jsonb_typeof(attributes->$k)='number' THEN (attributes->>$k)::double precision END`. | Single audited coercion; NULL result makes aggregates/filters/buckets all graceful for free; key bound once, referenced twice; fixed identifiers only. |

---

## 4. DSL design (`gauge-query`)

### 4.1 Dimensions — `Vec<Field>` → `Vec<Dimension>`

Untagged enum so existing `["app","event_name"]` parses unchanged:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Dimension {
    Field(Field),                  // "app", "attr.surface"
    Bucket { bucket: BucketSpec }, // {"bucket":{"field":"attr.latency_ms","edges":[50,200,500,1000]}}
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BucketSpec {
    pub field: Field,     // must be Attr(_)
    pub edges: Vec<f64>,  // non-empty, strictly ascending, finite, <= MAX_BUCKET_EDGES (32)
}
```

A `Dimension`'s output **alias** is the field's display string (`"app"`,
`"attr.latency_ms"`) — matching how text dimensions are already aliased. The
helper `Dimension::field()`/`alias()` centralises this.

### 4.2 Measures — add numeric aggregates

Untagged: simple measures keep their string wire form; aggregates are single-key
objects (externally-tagged), so `{"avg":"attr.latency_ms"}`, `{"p95":"attr.latency_ms"}`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Measure {
    Simple(SimpleMeasure),  // "count" | "unique_installs" | "unique_sessions"
    Agg(AggMeasure),
}

#[serde(rename_all = "snake_case")]
pub enum SimpleMeasure { Count, UniqueInstalls, UniqueSessions }

#[serde(rename_all = "snake_case")]  // externally tagged: {"avg": <field>}
pub enum AggMeasure { Avg(Field), Min(Field), Max(Field), P50(Field), P90(Field), P95(Field), P99(Field) }
```

- **Alias:** simple measures keep `count`/`unique_installs`/`unique_sessions`;
  aggregates → `{fn}_{key}` (e.g. `avg_latency_ms`, `p95_latency_ms`). Unique per
  (fn, field); order-by-able.
- `Measure::alias()` and a `Measure::numeric_field()` helper centralise this.

### 4.3 Filters — four comparison ops + a numeric value

```rust
#[serde(rename_all = "lowercase")]
pub enum FilterOp { Eq, Neq, In, Exists, Gt, Gte, Lt, Lte }

#[serde(untagged)]
pub enum FilterValue { One(String), Many(Vec<String>), Num(f64) }
```

`{"field":"attr.latency_ms","op":"gt","value":500}` → `FilterValue::Num(500.0)`.
(Variant order puts `One`/`Many` before `Num`; JSON number/string/array are
disjoint types so untagged resolution is unambiguous.)

### 4.4 Validation (`validate.rs`)

New `QueryError` variants and rules:

- `NumericFieldRequired(field)` — bucket field, aggregate field, and
  `gt/gte/lt/lte` filter field **must be `Attr(_)`**. (Envelope columns are text;
  `install_id`/`session_id` aren't `Field`s at all → stay non-addressable.)
- `BadBucketEdges(detail)` — edges empty, non-finite, not strictly ascending, or
  `> MAX_BUCKET_EDGES`.
- `DuplicateOutput(alias)` — two dimensions/measures resolve to the same output
  alias (e.g. a raw `attr.latency_ms` dimension **and** a bucket on the same field).
- `gt/gte/lt/lte` require `FilterValue::Num`; `eq/neq` require `One`; `in` requires
  non-empty `Many`; `exists` requires no value + `Attr`.

### 4.5 Response meta (`response.rs`)

Additive and backward-compatible (`skip_serializing_if = Option::is_none` →
existing responses are byte-identical when there are no buckets):

```rust
pub struct QueryResponse {
    pub rows: Vec<serde_json::Value>,
    pub truncated: bool,
    pub elapsed_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<QueryMeta>,
}
pub struct QueryMeta { pub buckets: Vec<BucketMeta> }
pub struct BucketMeta {
    pub field: String,        // "attr.latency_ms"
    pub alias: String,        // output key in rows (= field display string)
    pub edges: Vec<f64>,
    pub labels: Vec<String>,  // edges.len() + 1 entries
}
```

**Label format:** edges `[50,200,500,1000]` →
`["<50","50-200","200-500","500-1000","1000+"]`. Integer-valued edges are
formatted without a trailing `.0`; label generation lives in one helper
(`bucket_labels(&edges) -> Vec<String>`) shared by the server and reused for tests.

---

## 5. Server design (`gauge-server`)

### 5.1 The shared safe cast

```sql
CASE WHEN jsonb_typeof(attributes->$k) = 'number'
     THEN (attributes->>$k)::double precision END
```

- `$k` (the attr key) is **bound once and referenced twice** (Postgres allows
  reusing a placeholder); `jsonb_typeof`, `'number'`, `->`, `->>` are fixed.
- Non-numeric / string-encoded / absent → `NULL`. That single NULL makes every
  consumer graceful: SQL aggregates skip NULL; `<inner> > $v` is NULL→excluded; the
  bucket dimension adds `WHERE <inner> IS NOT NULL` so non-numeric rows don't form a
  spurious NULL group.
- Implemented as `numeric_field_expr(field, &mut binds) -> String` (distinct from
  the existing text `field_expr`, which pushes one bind per call).

### 5.2 SQL per capability

- **Bucket dimension:**
  `width_bucket(<inner>, $edges) AS "attr.latency_ms"`, with `$edges` bound as
  `Bind::FloatArr` (`double precision[]`). GROUP BY + ORDER BY the integer index;
  plus `WHERE <inner> IS NOT NULL`.
- **Aggregates:** `AVG(<inner>)`, `MIN(<inner>)`, `MAX(<inner>)`,
  `percentile_cont(0.95) WITHIN GROUP (ORDER BY <inner>)` — the fraction is a fixed
  literal per variant.
- **Comparison filters:** `<inner> > $v` / `>=` / `<` / `<=`, `$v` bound as
  `Bind::Float`.

### 5.3 Binds, column kinds, decode

```rust
enum Bind { Text(String), TextArr(Vec<String>), Time(OffsetDateTime),
            Float(f64), FloatArr(Vec<f64>) }            // + 2

enum ColKind { Text, Int, TimeBucket,
               Float,                                   // avg/min/max/percentiles
               Bucket { labels: Vec<String> } }         // width_bucket index -> label
```

`routes/query.rs` decode:
- `Float` → `try_get::<Option<f64>>` → JSON number / `null` (empty group → NULL).
- `Bucket { labels }` → `try_get::<Option<i32>>` → `labels[idx]` as a string.
- `BuiltQuery` carries `Vec<BucketMeta>` (labels computed once in `sqlbuild`); the
  route attaches `meta: Some(QueryMeta { buckets })` when non-empty, else `None`.

**Default order** extends to: `time_bucket ASC` (if granularity) **then each bucket
dimension ASC** — so a histogram comes out in range order without an explicit
`order`.

### 5.4 `get_meta` numeric discovery

`AppMeta` gains `numeric_attribute_keys: Vec<String>` (subset of
`attribute_keys`). The meta route detects keys whose values are JSON numbers
(`jsonb_typeof(value) = 'number'`) — implemented as an extension of the existing
attribute-key aggregation in `routes/meta.rs`.

### 5.5 Safety & snapshot tests

- Extend `user_values_never_appear_in_sql_text` to assert **edge values** and
  **numeric filter values** never appear in SQL text (use distinctive values, e.g.
  edges `[137, 911]`, and assert those substrings are absent).
- New snapshots: bucket SQL; aggregate SQL (mixing `avg` + `p95` + `count`);
  numeric-filter SQL.
- A decode-level test confirming `Bucket`/`Float` map correctly (index→label,
  NULL→null) — at the unit level where possible.

---

## 6. Client design

### 6.1 CLI (`query_cmd.rs` + README)

`gauge query '<json>'` is a raw passthrough — it inherits the capability the moment
the DSL compiles. Changes:
- A parse test exercising a bucket + aggregate + numeric-filter request.
- README "Query DSL" section updated: numeric measures, the bucket dimension,
  `gt/gte/lt/lte`, and the new `meta` field, with a worked latency-histogram +
  percentiles example.

### 6.2 MCP (`mcp/`)

- **`query_telemetry` tool description** (`server.rs`) extended to document numeric
  aggregates, the bucket dimension, `gt/gte/lt/lte`, and "non-numeric values are
  excluded."
- **`render.rs` `project_query`** stays generic (rows are `Value`s); it surfaces
  `meta` (already in the cloned structured content) and **suppresses** the
  count-only "view as trend" suggestion when an aggregate/bucket is present.
- **`numeric_stats`** — `tools.rs` builder + `render.rs` projector + `schemas.rs`
  output schema. Params `{ period, field, app?, event_name? }`; `field` is a bare
  attr key (e.g. `"latency_ms"`, internally `Field::Attr`). Builds measures
  `[avg, min, max, p50, p90, p95, p99]`; single-row result. Summary e.g.
  *"latency_ms (tome, 7d): avg 142, p95 480, max 1200."* `suggested_next_actions`
  links to `numeric_histogram`.
- **`numeric_histogram`** — params `{ period, field, edges, app?, event_name? }`;
  builds a single bucket dimension + `count` (and `unique_installs`). Projector
  reads `meta.labels` to summarise the distribution. Links back to `numeric_stats`.
- Update the MCP tool-count test (5 → **7**) and ensure both new tools advertise
  read-only annotations + described output schemas.

### 6.3 TUI (`tui/`)

Explore page (`app.rs`/`ui.rs`/`data.rs`) gains a numeric mode:
- A **numeric-attr axis** cycled from `meta.numeric_attribute_keys`.
- **Aggregate measures** (avg/min/max/p95) over the selected attr, rendered as a
  result table/line.
- A **histogram view**: on selecting a numeric attr, the TUI first runs a
  **min/max probe** query, derives ~6 nicely-rounded even edges across the observed
  range (`derive_edges(min, max) -> Vec<f64>`), then issues the bucket query and
  draws a horizontal bar chart using `meta.labels` for bar labels.
- Empty/over-narrow ranges degrade gracefully (single bucket / "no numeric data").

---

## 7. Safety invariants (unchanged, re-verified)

- SQL identifiers come **only** from closed enums / restricted-charset attr keys.
- **Every** user-supplied value — filter values, attr keys, **bucket edges**,
  numeric filter values — is a **bind parameter**, never string-spliced.
- `install_id` / `session_id` remain non-`Field`s → no per-install drill-down; they
  are still only countable via `unique_installs` / `unique_sessions`.
- Numeric ops are `attr.*`-only; percentile fractions and the typeof guard are fixed
  literals.

## 8. Testing strategy

- **`gauge-query`**: serde round-trip for new `Dimension`/`Measure`/`Filter` shapes
  (incl. backward-compat for existing string dimensions/measures); validation tests
  (numeric op on non-attr → error, bad/unsorted edges → error, duplicate alias →
  error); `bucket_labels` / edge-formatting unit tests.
- **`gauge-server`**: snapshot tests (bucket / aggregate+percentile / numeric
  filter); extended no-leak SQL-text test; decode mapping test.
- **`gauge` (clients)**: MCP builder tests (`numeric_stats`, `numeric_histogram`),
  projector tests, tool-count/schema test; CLI parse test; TUI `derive_edges` +
  numeric `explore_request` tests.
- **Docs**: README example kept honest (covered by `fact-check` if run).

## 9. Acceptance criteria

1. A query buckets a numeric `attr.*` by caller-supplied edges and returns
   per-bucket `count` / `unique_installs`, with labels + edges in `meta`.
2. `avg` / `min` / `max` / `p50` / `p90` / `p95` / `p99` work over a numeric `attr.*`.
3. `gt` / `gte` / `lt` / `lte` filters work over a numeric `attr.*`.
4. Non-numeric / null attribute values are excluded gracefully (no 500s).
5. Edges and numeric filter values never appear in SQL text (extends the existing
   `user_values_never_appear_in_sql_text` test).
6. Snapshot tests cover bucket + aggregate + numeric-filter SQL.
7. **CLI**: README documents the numeric DSL with a worked example; passthrough verified.
8. **MCP**: `query_telemetry` description covers the numeric DSL; `numeric_stats` and
   `numeric_histogram` exist with read-only annotations + output schemas; render
   surfaces `meta`.
9. **TUI**: Explore supports numeric aggregates and an auto-fit histogram over a
   numeric attr discovered from `get_meta`.
10. `get_meta` reports `numeric_attribute_keys`.

## 10. Rough implementation order

1. **`gauge-query`** — types, validation, `bucket_labels`, response `meta`,
   `AppMeta.numeric_attribute_keys` (foundation; everything depends on it).
2. **`gauge-server`** — `numeric_field_expr`, bucket/agg/filter SQL, new `Bind`/
   `ColKind`, `routes/query.rs` decode + meta, `routes/meta.rs` numeric detection,
   snapshots + safety test.
3. **CLI** — parse test + README.
4. **MCP** — description, `render.rs`, `numeric_stats`, `numeric_histogram`,
   schemas, tool-count test.
5. **TUI** — numeric attr axis, aggregate measures, min/max-probe histogram.

## 11. Out of scope / follow-ups

- Porting tome / midnight-manual to emit the numeric attrs this consumes (separate
  cycles per the kernel design).
- Arbitrary expression measures, multi-field buckets, log-scale auto-edges.
