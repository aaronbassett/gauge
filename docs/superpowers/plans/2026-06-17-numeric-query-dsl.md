# Numeric Query DSL Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add read-time numeric bucketing, aggregates (avg/min/max/p50/p90/p95/p99), and comparison filters (gt/gte/lt/lte) over numeric `attr.*` values to the query DSL, surfaced across the server, CLI, MCP, and TUI.

**Architecture:** All numeric values share one graceful JSONB→`double precision` cast (`CASE WHEN jsonb_typeof(attributes->$k)='number' THEN (attributes->>$k)::double precision END`) so non-numeric/null rows are excluded, never errored. Buckets use `width_bucket` (integer index drives SQL grouping/ordering; the wire row carries a human label; edges+labels are echoed in a new, optional `QueryResponse.meta`). The shared `gauge-query` DSL is changed first; each later task is a vertical slice that keeps the whole workspace green.

**Tech Stack:** Rust (edition 2024), `serde`/`schemars`, `sqlx` (Postgres), `insta` (snapshot tests), `rmcp` (MCP), `ratatui` (TUI).

**Spec:** `docs/superpowers/specs/2026-06-17-numeric-query-dsl-design.md`

---

## Conventions (read once)

- **Test a crate:** `cargo test -p gauge-query` · `cargo test -p gauge-server` · `cargo test -p gauge`
- **Lint before each commit:** `cargo clippy --workspace --all-targets -- -D warnings`
- **New/changed insta snapshots:** run the test (it will fail with a pending snapshot), then accept with `cargo insta accept` (install once with `cargo install cargo-insta` if missing) — or `INSTA_UPDATE=always cargo test -p gauge-server`. Always eyeball the `.snap` before committing.
- **Commit trailer:** every commit ends with
  `git commit -m "<type>(<scope>): <subject>" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"`
- **Baseline:** before Task 1, run `cargo test --workspace` and confirm it is green. The branch is `feat/numeric-query-dsl` (already cut from `main`, with the spec committed).

## File-structure map

| File | Responsibility | Change |
|---|---|---|
| `crates/gauge-query/src/request.rs` | DSL request types | `Measure` flat enum + hand serde + helpers; `FilterOp` +4 ops; `FilterValue::Num`; `Dimension`/`BucketSpec`; `dimensions: Vec<Dimension>` |
| `crates/gauge-query/src/validate.rs` | request validation | numeric-field rule, edge validation, dup-alias, filter arms; new `QueryError` variants |
| `crates/gauge-query/src/response.rs` | response types | `meta: Option<QueryMeta>`, `QueryMeta`, `BucketMeta` |
| `crates/gauge-query/src/meta.rs` | discovery types | `AppMeta.numeric_attribute_keys` |
| `crates/gauge-query/src/lib.rs` | re-exports | export new items + `bucket_labels` |
| `crates/gauge-server/src/sqlbuild.rs` | request→SQL | `numeric_field_expr`, agg/percentile/`width_bucket` SQL, `Bind::Float*`, `ColKind::Float`/`Bucket`, `BuiltQuery.bucket_meta`, decode helpers |
| `crates/gauge-server/src/routes/query.rs` | execute + decode | bind floats, decode `Float`/`Bucket`, attach `meta` |
| `crates/gauge-server/src/routes/meta.rs` | discovery query | numeric-key detection |
| `crates/gauge/src/mcp/{server,tools,render,schemas}.rs` | MCP surface | description, projector, two new tools, schemas |
| `crates/gauge/src/tui/{app,data,ui,run}.rs` | dashboard | numeric attr axis, aggregate measures, histogram + min/max probe |
| `README.md` | docs | Query DSL numeric section + example |

---

# Phase 1 — DSL + server

### Task 1: Numeric aggregate measures (avg/min/max/p50/p90/p95/p99)

**Files:**
- Modify: `crates/gauge-query/src/request.rs` (`Measure`)
- Modify: `crates/gauge-query/src/validate.rs` (`QueryError`, `validate`)
- Modify: `crates/gauge-query/src/lib.rs` (re-exports)
- Modify: `crates/gauge-server/src/sqlbuild.rs` (`numeric_field_expr`, `ColKind::Float`, `float_value`, measures loop)
- Modify: `crates/gauge-server/src/routes/query.rs` (decode `Float`)
- Modify: `crates/gauge/src/mcp/tools.rs` (`top_events_query` — `Measure` is no longer `Copy`)

- [ ] **Step 1: Write the failing serde round-trip test** in `request.rs` `#[cfg(test)]`:

```rust
#[test]
fn measure_serde_simple_and_aggregate() {
    use crate::request::Measure;
    // simple measures stay strings
    assert_eq!(serde_json::to_value(&Measure::Count).unwrap(), serde_json::json!("count"));
    let m: Measure = serde_json::from_value(serde_json::json!("unique_installs")).unwrap();
    assert_eq!(m, Measure::UniqueInstalls);
    // aggregates are single-key objects keyed by the agg name
    let avg: Measure = serde_json::from_value(serde_json::json!({"avg": "attr.latency_ms"})).unwrap();
    assert!(matches!(&avg, Measure::Avg(f) if f.to_string() == "attr.latency_ms"));
    assert_eq!(serde_json::to_value(&avg).unwrap(), serde_json::json!({"avg": "attr.latency_ms"}));
    let p95: Measure = serde_json::from_value(serde_json::json!({"p95": "attr.latency_ms"})).unwrap();
    assert_eq!(p95.alias(), "p95_latency_ms");
    // a two-key aggregate object is rejected
    assert!(serde_json::from_value::<Measure>(serde_json::json!({"avg":"attr.a","min":"attr.b"})).is_err());
}
```

- [ ] **Step 2: Run it to confirm it fails to compile** (no `Avg` variant yet)

Run: `cargo test -p gauge-query measure_serde_simple_and_aggregate`
Expected: FAIL — `no variant named Avg` / `alias` arity.

- [ ] **Step 3: Replace the `Measure` enum** in `request.rs`. Delete the existing `#[derive(... )] pub enum Measure { Count, UniqueInstalls, UniqueSessions }` and its `impl` and replace with:

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum Measure {
    Count,
    UniqueInstalls,
    UniqueSessions,
    Avg(Field),
    Min(Field),
    Max(Field),
    P50(Field),
    P90(Field),
    P95(Field),
    P99(Field),
}

impl Measure {
    /// Output column alias. Aggregates → `{fn}_{attr-key}` (e.g. `avg_latency_ms`).
    pub fn alias(&self) -> String {
        fn key(f: &Field) -> String {
            match f { Field::Attr(k) => k.clone(), other => other.to_string() }
        }
        match self {
            Measure::Count => "count".into(),
            Measure::UniqueInstalls => "unique_installs".into(),
            Measure::UniqueSessions => "unique_sessions".into(),
            Measure::Avg(f) => format!("avg_{}", key(f)),
            Measure::Min(f) => format!("min_{}", key(f)),
            Measure::Max(f) => format!("max_{}", key(f)),
            Measure::P50(f) => format!("p50_{}", key(f)),
            Measure::P90(f) => format!("p90_{}", key(f)),
            Measure::P95(f) => format!("p95_{}", key(f)),
            Measure::P99(f) => format!("p99_{}", key(f)),
        }
    }
    /// The numeric attr field an aggregate operates on, if any.
    pub fn numeric_field(&self) -> Option<&Field> {
        match self {
            Measure::Avg(f) | Measure::Min(f) | Measure::Max(f)
            | Measure::P50(f) | Measure::P90(f) | Measure::P95(f) | Measure::P99(f) => Some(f),
            _ => None,
        }
    }
}

impl serde::Serialize for Measure {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let (name, field) = match self {
            Measure::Count => return s.serialize_str("count"),
            Measure::UniqueInstalls => return s.serialize_str("unique_installs"),
            Measure::UniqueSessions => return s.serialize_str("unique_sessions"),
            Measure::Avg(f) => ("avg", f),
            Measure::Min(f) => ("min", f),
            Measure::Max(f) => ("max", f),
            Measure::P50(f) => ("p50", f),
            Measure::P90(f) => ("p90", f),
            Measure::P95(f) => ("p95", f),
            Measure::P99(f) => ("p99", f),
        };
        let mut m = s.serialize_map(Some(1))?;
        m.serialize_entry(name, field)?;
        m.end()
    }
}

impl<'de> serde::Deserialize<'de> for Measure {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = Measure;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a measure name or a single-key aggregate object")
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Measure, E> {
                match v {
                    "count" => Ok(Measure::Count),
                    "unique_installs" => Ok(Measure::UniqueInstalls),
                    "unique_sessions" => Ok(Measure::UniqueSessions),
                    other => Err(E::custom(format!("unknown measure `{other}`"))),
                }
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Measure, A::Error> {
                let entry: Option<(String, Field)> = map.next_entry()?;
                let (name, field) = entry
                    .ok_or_else(|| serde::de::Error::custom("empty aggregate measure object"))?;
                if map.next_key::<String>()?.is_some() {
                    return Err(serde::de::Error::custom("aggregate measure must have exactly one key"));
                }
                match name.as_str() {
                    "avg" => Ok(Measure::Avg(field)),
                    "min" => Ok(Measure::Min(field)),
                    "max" => Ok(Measure::Max(field)),
                    "p50" => Ok(Measure::P50(field)),
                    "p90" => Ok(Measure::P90(field)),
                    "p95" => Ok(Measure::P95(field)),
                    "p99" => Ok(Measure::P99(field)),
                    other => Err(serde::de::Error::custom(format!("unknown aggregate `{other}`"))),
                }
            }
        }
        d.deserialize_any(V)
    }
}

impl schemars::JsonSchema for Measure {
    fn schema_name() -> std::borrow::Cow<'static, str> { "Measure".into() }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "A simple measure name, or a single-key aggregate object over a numeric attr.<key>.",
            "oneOf": [
                { "type": "string", "enum": ["count", "unique_installs", "unique_sessions"] },
                { "type": "object", "minProperties": 1, "maxProperties": 1, "additionalProperties": false,
                  "properties": {
                    "avg": {"type":"string"}, "min": {"type":"string"}, "max": {"type":"string"},
                    "p50": {"type":"string"}, "p90": {"type":"string"}, "p95": {"type":"string"}, "p99": {"type":"string"}
                  } }
            ]
        })
    }
}
```

- [ ] **Step 4: Run the gauge-query test to confirm it passes**

Run: `cargo test -p gauge-query measure_serde_simple_and_aggregate`
Expected: PASS.

- [ ] **Step 5: Add the validation rule + error variant.** In `validate.rs`, add to `enum QueryError`:

```rust
    #[error("numeric operation on `{0}` requires an attr.<key> field")]
    NumericFieldRequired(String),
```

In `fn validate`, immediately after the `EmptyMeasures` check, add:

```rust
    for m in &req.measures {
        if let Some(f) = m.numeric_field()
            && !matches!(f, Field::Attr(_))
        {
            return Err(QueryError::NumericFieldRequired(f.to_string()));
        }
    }
```

(`Measure` is imported via `use crate::request::{...}`; add `Measure` to that import list.)

- [ ] **Step 6: Add a validation test** in `validate.rs` tests (create a `#[cfg(test)] mod tests` if absent — there is none today, so add one):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::{Measure, QueryRequest};

    fn req_with(measures: Vec<Measure>) -> QueryRequest {
        QueryRequest {
            measures,
            dimensions: vec![],
            filters: vec![],
            time_range: crate::request::TimeRange::Last { last: "1d".into() },
            granularity: None,
            order: vec![],
            limit: None,
        }
    }

    #[test]
    fn aggregate_requires_attr_field() {
        let ok = req_with(vec![Measure::Avg(Field::Attr("latency_ms".into()))]);
        assert!(validate(&ok).is_ok());
        let bad = req_with(vec![Measure::Avg(Field::Os)]);
        assert!(matches!(validate(&bad), Err(QueryError::NumericFieldRequired(_))));
    }
}
```

Run: `cargo test -p gauge-query` → Expected: PASS.

- [ ] **Step 7: Add `numeric_field_expr`, `ColKind::Float`, and `float_value` to `sqlbuild.rs`.** Add `use serde_json::Value;` near the top imports. Add the `Float` variant to `enum ColKind` (after `TimeBucket`). Add these functions next to `field_expr`:

```rust
/// Graceful JSONB→f64 cast: non-numeric / absent values become NULL (excluded
/// from aggregates/filters; never an error). The key is bound once and the
/// placeholder is referenced twice.
fn numeric_field_expr(f: &Field, binds: &mut Vec<Bind>) -> String {
    let Field::Attr(k) = f else {
        unreachable!("validated: numeric ops require an attr.<key> field")
    };
    let p = ph(binds, Bind::Text(k.clone()));
    format!("CASE WHEN jsonb_typeof(attributes->{p}) = 'number' THEN (attributes->>{p})::double precision END")
}

fn percentile_expr(f: &Field, frac: &str, binds: &mut Vec<Bind>) -> String {
    // `frac` is a fixed literal per measure variant — never user input.
    format!("percentile_cont({frac}) WITHIN GROUP (ORDER BY {})", numeric_field_expr(f, binds))
}

/// Decode an `f64` column (NULL → JSON null; non-finite → null).
pub fn float_value(v: Option<f64>) -> Value {
    v.and_then(serde_json::Number::from_f64).map(Value::Number).unwrap_or(Value::Null)
}
```

- [ ] **Step 8: Replace the measures loop** in `sqlbuild::build` (currently the `for m in &req.measures { ... }` block) with:

```rust
    for m in &req.measures {
        let (expr, kind) = match m {
            Measure::Count => ("COUNT(*)".to_string(), ColKind::Int),
            Measure::UniqueInstalls => ("COUNT(DISTINCT install_id)".to_string(), ColKind::Int),
            Measure::UniqueSessions => ("COUNT(DISTINCT session_id)".to_string(), ColKind::Int),
            Measure::Avg(f) => (format!("AVG({})", numeric_field_expr(f, &mut binds)), ColKind::Float),
            Measure::Min(f) => (format!("MIN({})", numeric_field_expr(f, &mut binds)), ColKind::Float),
            Measure::Max(f) => (format!("MAX({})", numeric_field_expr(f, &mut binds)), ColKind::Float),
            Measure::P50(f) => (percentile_expr(f, "0.5", &mut binds), ColKind::Float),
            Measure::P90(f) => (percentile_expr(f, "0.9", &mut binds), ColKind::Float),
            Measure::P95(f) => (percentile_expr(f, "0.95", &mut binds), ColKind::Float),
            Measure::P99(f) => (percentile_expr(f, "0.99", &mut binds), ColKind::Float),
        };
        let alias = m.alias();
        select.push(format!("{expr} AS \"{alias}\""));
        columns.push(ColSpec { alias, kind });
    }
```

- [ ] **Step 9: Decode the Float column.** In `routes/query.rs`, change `let v = match col.kind {` to `let v = match &col.kind {` and add an arm after `ColKind::TimeBucket => ...`:

```rust
                ColKind::Float => row
                    .try_get::<Option<f64>, _>(col.alias.as_str())
                    .map(sqlbuild::float_value),
```

(`sqlbuild::float_value` is already imported via `use crate::sqlbuild::{self, ...}` — it is, `self` is in scope.)

- [ ] **Step 10: Fix the `Measure`-is-no-longer-`Copy` site** in `mcp/tools.rs` `top_events_query`. Replace the `QueryRequest { ... }` construction so the alias is computed before `measure` is moved:

```rust
    let measure = match p.by.unwrap_or(TopBy::Count) {
        TopBy::Count => Measure::Count,
        TopBy::UniqueInstalls => Measure::UniqueInstalls,
    };
    let order_field = measure.alias();
    QueryRequest {
        measures: vec![measure],
        dimensions: vec![Field::EventName],
        filters: base_filters(&p.app, &None),
        time_range: TimeRange::Last { last: p.period.clone() },
        granularity: None,
        order: vec![Order { field: order_field, dir: Dir::Desc }],
        limit: Some(p.limit.unwrap_or(10)),
    }
```

- [ ] **Step 11: Add a snapshot test for aggregate SQL** in `sqlbuild.rs` tests:

```rust
    #[test]
    fn snapshot_aggregate_measures() {
        let req: QueryRequest = serde_json::from_str(
            r#"{"measures":["count",{"avg":"attr.latency_ms"},{"p95":"attr.latency_ms"}],
                "dimensions":["app"],"time_range":{"last":"7d"}}"#,
        ).unwrap();
        insta::assert_snapshot!(build(&req, NOW).unwrap().sql);
    }
```

- [ ] **Step 12: Run tests, accept the snapshot, lint**

Run: `cargo test -p gauge-server snapshot_aggregate_measures` → pending snapshot.
Run: `cargo insta accept` then re-run; eyeball the `.snap` shows `AVG(CASE WHEN jsonb_typeof(...)='number' THEN ... END) AS "avg_latency_ms"` and `percentile_cont(0.95) WITHIN GROUP (...) AS "p95_latency_ms"`.
Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 13: Commit**

```bash
git add crates/gauge-query crates/gauge-server crates/gauge/src/mcp/tools.rs
git commit -m "feat(query): numeric aggregate measures (avg/min/max/percentiles)" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 2: Numeric comparison filters (gt/gte/lt/lte)

**Files:**
- Modify: `crates/gauge-query/src/request.rs` (`FilterOp`, `FilterValue`)
- Modify: `crates/gauge-query/src/validate.rs` (filter arms)
- Modify: `crates/gauge-server/src/sqlbuild.rs` (`Bind::Float`, filter arms)
- Modify: `crates/gauge-server/src/routes/query.rs` (bind `Float`)

- [ ] **Step 1: Write the failing validation + serde test** in `validate.rs` tests:

```rust
    #[test]
    fn numeric_filter_requires_num_and_attr() {
        use crate::request::{Filter, FilterOp, FilterValue};
        let mut r = req_with(vec![Measure::Count]);
        r.filters = vec![Filter {
            field: Field::Attr("latency_ms".into()),
            op: FilterOp::Gt,
            value: Some(FilterValue::Num(500.0)),
        }];
        assert!(validate(&r).is_ok());
        // gt on a non-attr field is rejected
        r.filters[0].field = Field::Os;
        assert!(matches!(validate(&r), Err(QueryError::NumericFieldRequired(_))));
        // gt with a string value is rejected
        r.filters[0].field = Field::Attr("latency_ms".into());
        r.filters[0].value = Some(FilterValue::One("500".into()));
        assert!(matches!(validate(&r), Err(QueryError::BadFilter(..))));
    }
```

- [ ] **Step 2: Run to confirm failure** (no `FilterOp::Gt`)

Run: `cargo test -p gauge-query numeric_filter_requires_num_and_attr`
Expected: FAIL — `no variant named Gt`.

- [ ] **Step 3: Extend `FilterOp` and `FilterValue`** in `request.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FilterOp { Eq, Neq, In, Exists, Gt, Gte, Lt, Lte }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum FilterValue {
    One(String),
    Many(Vec<String>),
    Num(f64),
}
```

(Variant order matters for untagged deserialization: a JSON string → `One`, array → `Many`, number → `Num`.)

- [ ] **Step 4: Add the validation arms.** In `validate.rs`, inside the `match (f.op, &f.value)` block, add these arms **before** the existing `(FilterOp::Eq | FilterOp::Neq, _)` catch-all:

```rust
            (FilterOp::Gt | FilterOp::Gte | FilterOp::Lt | FilterOp::Lte, Some(FilterValue::Num(_))) => {
                if !matches!(f.field, Field::Attr(_)) {
                    return Err(QueryError::NumericFieldRequired(fname));
                }
            }
            (FilterOp::Gt | FilterOp::Gte | FilterOp::Lt | FilterOp::Lte, _) => {
                return Err(QueryError::BadFilter(
                    fname, opname, "a numeric value on an attr.<key> field",
                ));
            }
```

- [ ] **Step 5: Run the gauge-query tests**

Run: `cargo test -p gauge-query` → Expected: PASS.

- [ ] **Step 6: Add `Bind::Float` and the server filter arms.** In `sqlbuild.rs`, add `Float(f64)` to `enum Bind` (after `Time`). In `build`, add these arms to the `match (f.op, f.value.as_ref())` block **before** `_ => unreachable!`:

```rust
            (FilterOp::Gt, Some(FilterValue::Num(v))) => {
                let expr = numeric_field_expr(&f.field, &mut binds);
                let p = ph(&mut binds, Bind::Float(*v));
                wheres.push(format!("{expr} > {p}"));
            }
            (FilterOp::Gte, Some(FilterValue::Num(v))) => {
                let expr = numeric_field_expr(&f.field, &mut binds);
                let p = ph(&mut binds, Bind::Float(*v));
                wheres.push(format!("{expr} >= {p}"));
            }
            (FilterOp::Lt, Some(FilterValue::Num(v))) => {
                let expr = numeric_field_expr(&f.field, &mut binds);
                let p = ph(&mut binds, Bind::Float(*v));
                wheres.push(format!("{expr} < {p}"));
            }
            (FilterOp::Lte, Some(FilterValue::Num(v))) => {
                let expr = numeric_field_expr(&f.field, &mut binds);
                let p = ph(&mut binds, Bind::Float(*v));
                wheres.push(format!("{expr} <= {p}"));
            }
```

- [ ] **Step 7: Bind floats at execution.** In `routes/query.rs`, add to the `q = match b { ... }` block:

```rust
            Bind::Float(f) => q.bind(*f),
```

- [ ] **Step 8: Add a numeric-filter snapshot + extend the no-leak test** in `sqlbuild.rs` tests:

```rust
    #[test]
    fn snapshot_numeric_filter() {
        let req: QueryRequest = serde_json::from_str(
            r#"{"measures":["count"],
                "filters":[{"field":"attr.latency_ms","op":"gt","value":4242}],
                "time_range":{"last":"7d"}}"#,
        ).unwrap();
        let built = build(&req, NOW).unwrap();
        // the numeric filter value is bound, never spliced into SQL text
        assert!(!built.sql.contains("4242"));
        insta::assert_snapshot!(built.sql);
    }
```

- [ ] **Step 9: Run, accept snapshot, lint, commit**

Run: `cargo test -p gauge-server snapshot_numeric_filter` → pending; `cargo insta accept`; re-run.
Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings` → PASS.

```bash
git add crates/gauge-query crates/gauge-server
git commit -m "feat(query): numeric comparison filters (gt/gte/lt/lte)" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 3: Numeric bucket dimension + response meta

**Files:**
- Modify: `crates/gauge-query/src/request.rs` (`Dimension`, `BucketSpec`, `MAX_BUCKET_EDGES`, `dimensions` type)
- Modify: `crates/gauge-query/src/validate.rs` (edges + dup-alias, `bucket_labels`)
- Modify: `crates/gauge-query/src/response.rs` (`meta`, `QueryMeta`, `BucketMeta`)
- Modify: `crates/gauge-query/src/lib.rs` (re-exports)
- Modify: `crates/gauge-server/src/sqlbuild.rs` (`ColKind::Bucket`, `bucket_value`, `BuiltQuery.bucket_meta`, `Bind::FloatArr`, dimensions loop, default order)
- Modify: `crates/gauge-server/src/routes/query.rs` (bind `FloatArr`, decode `Bucket`, attach `meta`)
- Modify: `crates/gauge/src/tui/data.rs` and `crates/gauge/src/mcp/tools.rs` (wrap `Field` dimensions as `Dimension::Field`)

- [ ] **Step 1: Write the failing `bucket_labels` test** in `validate.rs` tests:

```rust
    #[test]
    fn bucket_labels_span_edges() {
        let labels = crate::validate::bucket_labels(&[50.0, 200.0, 500.0, 1000.0]);
        assert_eq!(labels, vec!["<50", "50-200", "200-500", "500-1000", "1000+"]);
        // fractional edges keep their decimals
        assert_eq!(crate::validate::bucket_labels(&[1.5, 3.0]), vec!["<1.5", "1.5-3", "3+"]);
    }
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test -p gauge-query bucket_labels_span_edges`
Expected: FAIL — `bucket_labels` not found.

- [ ] **Step 3: Add `bucket_labels` to `validate.rs`** (top-level `pub fn`):

```rust
/// Human-readable bucket labels for `width_bucket` indices 0..=edges.len().
/// e.g. `[50,200,500,1000]` → `["<50","50-200","200-500","500-1000","1000+"]`.
pub fn bucket_labels(edges: &[f64]) -> Vec<String> {
    fn fmt(x: f64) -> String {
        if x.fract() == 0.0 && x.abs() < 1e15 { format!("{}", x as i64) } else { format!("{x}") }
    }
    let mut labels = Vec::with_capacity(edges.len() + 1);
    if let Some(first) = edges.first() {
        labels.push(format!("<{}", fmt(*first)));
    }
    for w in edges.windows(2) {
        labels.push(format!("{}-{}", fmt(w[0]), fmt(w[1])));
    }
    if let Some(last) = edges.last() {
        labels.push(format!("{}+", fmt(*last)));
    }
    labels
}
```

Run: `cargo test -p gauge-query bucket_labels_span_edges` → PASS.

- [ ] **Step 4: Add `Dimension`, `BucketSpec`, `MAX_BUCKET_EDGES` and switch the field type** in `request.rs`. Add near `DEFAULT_LIMIT`:

```rust
pub const MAX_BUCKET_EDGES: usize = 32;
```

Change `QueryRequest.dimensions` from `Vec<Field>` to `Vec<Dimension>`. Add:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Dimension {
    Field(Field),                   // "app", "attr.surface"
    Bucket { bucket: BucketSpec },  // {"bucket":{"field":"attr.latency_ms","edges":[50,200,500,1000]}}
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BucketSpec {
    pub field: Field,
    pub edges: Vec<f64>,
}

impl Dimension {
    pub fn field(&self) -> &Field {
        match self {
            Dimension::Field(f) => f,
            Dimension::Bucket { bucket } => &bucket.field,
        }
    }
    /// Output column alias (the field's display string).
    pub fn alias(&self) -> String {
        self.field().to_string()
    }
}

impl From<Field> for Dimension {
    fn from(f: Field) -> Self { Dimension::Field(f) }
}
```

- [ ] **Step 5: Add a dimension serde + backward-compat test** in `request.rs` tests:

```rust
    #[test]
    fn dimension_serde_field_and_bucket() {
        use crate::request::{Dimension, QueryRequest};
        // a bare string is a Field dimension (backward compatible)
        let f: Dimension = serde_json::from_value(serde_json::json!("app")).unwrap();
        assert!(matches!(f, Dimension::Field(_)));
        // an object is a bucket dimension
        let b: Dimension = serde_json::from_value(
            serde_json::json!({"bucket": {"field": "attr.latency_ms", "edges": [50, 200]}})
        ).unwrap();
        assert!(matches!(b, Dimension::Bucket { .. }));
        // existing requests with string dimensions still parse
        let _: QueryRequest = serde_json::from_str(
            r#"{"measures":["count"],"dimensions":["app","event_name"],"time_range":{"last":"1d"}}"#
        ).unwrap();
    }
```

Run: `cargo test -p gauge-query dimension_serde_field_and_bucket` → PASS (this also forces fixing `Vec<Field>`→`Vec<Dimension>` mismatches inside `gauge-query`, e.g. the `req_with` helper, which already passes `vec![]`).

- [ ] **Step 6: Add edge + duplicate-alias validation.** In `validate.rs` add error variants:

```rust
    #[error("bucket edges on `{0}` must be non-empty, finite, strictly ascending, and at most {MAX_BUCKET_EDGES}")]
    BadBucketEdges(String),
    #[error("duplicate output `{0}` (two dimensions/measures resolve to the same column)")]
    DuplicateOutput(String),
```

Add `MAX_BUCKET_EDGES` and `Dimension` to the `use crate::request::{...}` import. In `fn validate`, after the measures numeric-field loop, add:

```rust
    use std::collections::HashSet;
    let mut seen: HashSet<String> = HashSet::new();
    for d in &req.dimensions {
        if let Dimension::Bucket { bucket } = d {
            if !matches!(bucket.field, Field::Attr(_)) {
                return Err(QueryError::NumericFieldRequired(bucket.field.to_string()));
            }
            let e = &bucket.edges;
            let ok = !e.is_empty()
                && e.len() <= MAX_BUCKET_EDGES
                && e.iter().all(|x| x.is_finite())
                && e.windows(2).all(|w| w[0] < w[1]);
            if !ok {
                return Err(QueryError::BadBucketEdges(bucket.field.to_string()));
            }
        }
        if !seen.insert(d.alias()) {
            return Err(QueryError::DuplicateOutput(d.alias()));
        }
    }
    for m in &req.measures {
        if !seen.insert(m.alias()) {
            return Err(QueryError::DuplicateOutput(m.alias()));
        }
    }
```

- [ ] **Step 7: Add a bucket-validation test** in `validate.rs` tests, then run:

```rust
    #[test]
    fn bucket_edges_must_be_sorted_and_attr() {
        use crate::request::{BucketSpec, Dimension};
        let mut r = req_with(vec![Measure::Count]);
        r.dimensions = vec![Dimension::Bucket { bucket: BucketSpec {
            field: Field::Attr("latency_ms".into()), edges: vec![200.0, 50.0],
        }}];
        assert!(matches!(validate(&r), Err(QueryError::BadBucketEdges(_))));
        r.dimensions[0] = Dimension::Bucket { bucket: BucketSpec {
            field: Field::App, edges: vec![50.0, 200.0],
        }};
        assert!(matches!(validate(&r), Err(QueryError::NumericFieldRequired(_))));
    }
```

Run: `cargo test -p gauge-query` → PASS.

- [ ] **Step 8: Add response meta types** in `response.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueryResponse {
    pub rows: Vec<serde_json::Value>,
    pub truncated: bool,
    pub elapsed_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<QueryMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueryMeta {
    pub buckets: Vec<BucketMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BucketMeta {
    pub field: String,
    pub alias: String,
    pub edges: Vec<f64>,
    pub labels: Vec<String>,
}
```

- [ ] **Step 9: Update `lib.rs` re-exports.** Add `Dimension`, `BucketSpec`, `MAX_BUCKET_EDGES` to the `request::{...}` re-export; add `QueryMeta, BucketMeta` to the `response::{...}` re-export; add `bucket_labels` to the `validate::{...}` re-export:

```rust
pub use request::{
    BucketSpec, DEFAULT_LIMIT, Dimension, Dir, Filter, FilterOp, FilterValue, Granularity,
    MAX_BUCKET_EDGES, MAX_LIMIT, Measure, Order, QueryRequest, TimeRange,
};
pub use response::{BucketMeta, QueryMeta, QueryResponse};
pub use validate::{QueryError, bucket_labels, parse_last, resolve_time_range, validate};
```

Run: `cargo test -p gauge-query && cargo build -p gauge-query` → PASS.

- [ ] **Step 10: Server — add `ColKind::Bucket`, `Bind::FloatArr`, `bucket_value`, `BuiltQuery.bucket_meta`.** In `sqlbuild.rs`:
  - Add `FloatArr(Vec<f64>)` to `enum Bind`.
  - Add `Bucket { labels: Vec<String> }` to `enum ColKind`.
  - Import `BucketMeta`, `Dimension`: extend the `use gauge_query::{...}` list with `BucketMeta, Dimension, bucket_labels`.
  - Add `pub bucket_meta: Vec<BucketMeta>` to `struct BuiltQuery`.
  - Add the decode helper:

```rust
/// Decode a `width_bucket` index column to its human label.
pub fn bucket_value(labels: &[String], idx: Option<i32>) -> Value {
    match idx {
        Some(i) if i >= 0 && (i as usize) < labels.len() => Value::String(labels[i as usize].clone()),
        _ => Value::Null,
    }
}
```

- [ ] **Step 11: Rewrite the dimensions loop and default order** in `sqlbuild::build`. First add `let mut bucket_meta: Vec<BucketMeta> = Vec::new();` near the other `let mut` locals. Replace the `for d in &req.dimensions { ... }` block with:

```rust
    for d in &req.dimensions {
        match d {
            Dimension::Field(f) => {
                let alias = f.to_string();
                let expr = field_expr(f, &mut binds);
                select.push(format!("{expr} AS \"{alias}\""));
                columns.push(ColSpec { alias, kind: ColKind::Text });
                group_count += 1;
            }
            Dimension::Bucket { bucket } => {
                let alias = bucket.field.to_string();
                let inner = numeric_field_expr(&bucket.field, &mut binds); // key bound once
                wheres.push(format!("{inner} IS NOT NULL")); // exclude non-numeric rows
                let pe = ph(&mut binds, Bind::FloatArr(bucket.edges.clone()));
                select.push(format!("width_bucket({inner}, {pe}) AS \"{alias}\""));
                let labels = bucket_labels(&bucket.edges);
                columns.push(ColSpec { alias: alias.clone(), kind: ColKind::Bucket { labels: labels.clone() } });
                bucket_meta.push(BucketMeta {
                    field: alias.clone(),
                    alias,
                    edges: bucket.edges.clone(),
                    labels,
                });
                group_count += 1;
            }
        }
    }
```

Then extend the default-order fallback. Replace the `if req.order.is_empty() { if req.granularity.is_some() { vec!["\"time_bucket\" ASC".into()] } else { vec![] } }` branch with:

```rust
    let order_terms: Vec<String> = if req.order.is_empty() {
        let mut terms = Vec::new();
        if req.granularity.is_some() {
            terms.push("\"time_bucket\" ASC".to_string());
        }
        for b in &bucket_meta {
            terms.push(format!("\"{}\" ASC", b.alias)); // bucket alias = width_bucket index → numeric order
        }
        terms
    } else {
        // leave the existing `req.order.iter().map(...).collect::<Result<_, _>>()?`
        // explicit-order branch exactly as-is
```

Finally, add `bucket_meta,` to the returned `BuiltQuery { ... }` literal.

- [ ] **Step 12: Decode the Bucket column and attach meta** in `routes/query.rs`. Add this arm to the `match &col.kind` block:

```rust
                ColKind::Bucket { labels } => row
                    .try_get::<Option<i32>, _>(col.alias.as_str())
                    .map(|i| sqlbuild::bucket_value(labels, i)),
```

Change the final `Ok(Json(QueryResponse { ... }))` to build `meta` from `built.bucket_meta`:

```rust
    let meta = if built.bucket_meta.is_empty() {
        None
    } else {
        Some(gauge_query::QueryMeta { buckets: built.bucket_meta.clone() })
    };
    Ok(Json(QueryResponse { rows: out, truncated, elapsed_ms: started.elapsed().as_millis() as u64, meta }))
```

Add `Bind::FloatArr(v) => q.bind(v),` to the bind `match` block.

- [ ] **Step 13: Wrap `Field` dimensions at the remaining construction sites.** In `tui/data.rs` change the three `dimensions: vec![Field::App]` / `vec![Field::EventName]` to `vec![Dimension::Field(Field::App)]` / `vec![Dimension::Field(Field::EventName)]` and add `Dimension` to the `use gauge_query::{...}` import. In `mcp/tools.rs` `top_events_query`, change `dimensions: vec![Field::EventName]` to `dimensions: vec![Dimension::Field(Field::EventName)]` and add `Dimension` to its import.

- [ ] **Step 14: Add a bucket snapshot + no-leak assertion** in `sqlbuild.rs` tests:

```rust
    #[test]
    fn snapshot_numeric_bucket() {
        let req: QueryRequest = serde_json::from_str(
            r#"{"measures":["count","unique_installs"],
                "dimensions":[{"bucket":{"field":"attr.latency_ms","edges":[137,911]}}],
                "time_range":{"last":"7d"}}"#,
        ).unwrap();
        let built = build(&req, NOW).unwrap();
        assert!(!built.sql.contains("137") && !built.sql.contains("911")); // edges bound, not spliced
        assert_eq!(built.bucket_meta.len(), 1);
        assert_eq!(built.bucket_meta[0].labels, vec!["<137", "137-911", "911+"]);
        insta::assert_snapshot!(built.sql);
    }

    #[test]
    fn bucket_value_maps_index_to_label() {
        let labels = vec!["<50".to_string(), "50-200".to_string(), "200+".to_string()];
        assert_eq!(bucket_value(&labels, Some(1)), serde_json::json!("50-200"));
        assert_eq!(bucket_value(&labels, None), serde_json::Value::Null);
        assert_eq!(bucket_value(&labels, Some(9)), serde_json::Value::Null);
    }
```

- [ ] **Step 15: Run, accept snapshot, lint, commit**

Run: `cargo test -p gauge-server snapshot_numeric_bucket` → pending; `cargo insta accept`; eyeball `.snap` (`width_bucket(CASE ... END, $N) AS "attr.latency_ms" ... AND CASE ... IS NOT NULL ... GROUP BY 1 ORDER BY "attr.latency_ms" ASC`).
Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings` → PASS.

```bash
git add crates/gauge-query crates/gauge-server crates/gauge/src/tui/data.rs crates/gauge/src/mcp/tools.rs
git commit -m "feat(query): numeric bucket dimension with width_bucket + response meta" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 4: `get_meta` numeric attribute discovery

**Files:**
- Modify: `crates/gauge-query/src/meta.rs` (`AppMeta.numeric_attribute_keys`)
- Modify: `crates/gauge-server/src/routes/meta.rs` (numeric-key query)

- [ ] **Step 1: Write the failing schema test** in `meta.rs` (add a `#[cfg(test)] mod tests`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn appmeta_has_numeric_attribute_keys() {
        let schema = serde_json::to_value(schemars::schema_for!(AppMeta)).unwrap();
        assert!(schema["properties"]["numeric_attribute_keys"].is_object());
    }
}
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test -p gauge-query appmeta_has_numeric_attribute_keys`
Expected: FAIL — property absent.

- [ ] **Step 3: Add the field** to `struct AppMeta` in `meta.rs`, after `attribute_keys`:

```rust
    /// Subset of `attribute_keys` whose values are JSON numbers (bucketable/aggregatable).
    #[serde(default)]
    pub numeric_attribute_keys: Vec<String>,
```

Run: `cargo test -p gauge-query appmeta_has_numeric_attribute_keys` → PASS.

- [ ] **Step 4: Populate it in the server meta route.** In `routes/meta.rs`, after the `keys` query, add:

```rust
    let numeric_keys = sqlx::query(
        "SELECT DISTINCT app, e.key AS key \
         FROM events, jsonb_each(attributes) AS e(key, value) \
         WHERE jsonb_typeof(e.value) = 'number' ORDER BY 1, 2",
    )
    .fetch_all(&st.pool)
    .await
    .map_err(db_err)?;
```

In the `AppMeta { ... }` initializer add `numeric_attribute_keys: vec![],`. After the existing `for row in &keys { ... }` loop add:

```rust
    for row in &numeric_keys {
        let app: String = row.get("app");
        if let Some(m) = apps.get_mut(&app) {
            m.numeric_attribute_keys.push(row.get("key"));
        }
    }
```

- [ ] **Step 5: Build, lint, commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings` → PASS.

```bash
git add crates/gauge-query/src/meta.rs crates/gauge-server/src/routes/meta.rs
git commit -m "feat(meta): report numeric_attribute_keys for discovery" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

# Phase 2 — clients

### Task 5: CLI passthrough test + README docs

**Files:**
- Modify: `crates/gauge/src/query_cmd.rs` (test)
- Modify: `README.md` (Query DSL section)

- [ ] **Step 1: Add a CLI parse test** in `query_cmd.rs` tests:

```rust
    #[test]
    fn accepts_numeric_bucket_aggregate_and_filter() {
        parse_request(
            r#"{"measures":[{"avg":"attr.latency_ms"},{"p95":"attr.latency_ms"}],
                "dimensions":[{"bucket":{"field":"attr.latency_ms","edges":[50,200,500,1000]}}],
                "filters":[{"field":"attr.latency_ms","op":"gt","value":0}],
                "time_range":{"last":"7d"}}"#,
        ).unwrap();
    }
```

Run: `cargo test -p gauge accepts_numeric_bucket_aggregate_and_filter` → PASS (passthrough already works).

- [ ] **Step 2: Update the README Query DSL section.** Replace the `**Measures:**`, `**Dimensions:**`, and `**Filters:**` bullets (around `README.md:239-241`) with:

```markdown
- **Measures:** `count`, `unique_installs` (`COUNT(DISTINCT install_id)`), `unique_sessions`; numeric aggregates over a numeric `attr.<key>` as single-key objects — `{"avg":"attr.latency_ms"}`, `min`, `max`, `p50`, `p90`, `p95`, `p99`
- **Dimensions:** `app`, `event_name`, `app_version`, `os`, `arch`, `attr.<key>`, a time bucket when `granularity` is set, plus a numeric **bucket** of a numeric `attr.<key>`: `{"bucket":{"field":"attr.latency_ms","edges":[50,200,500,1000]}}` (rows carry the range label; edges + labels are echoed in `meta.buckets`)
- **Filters:** `eq` · `neq` · `in` · `exists` over any field; numeric `gt` · `gte` · `lt` · `lte` over a numeric `attr.<key>` (e.g. `{"field":"attr.latency_ms","op":"gt","value":500}`). Non-numeric / null attribute values are excluded, never errored.
```

- [ ] **Step 3: Add a worked numeric example** after the existing query example fence (around `README.md:265`):

```markdown
A latency histogram with percentiles:

​```jsonc
// POST /v1/query
{
  "measures": ["count", {"p95":"attr.latency_ms"}],
  "dimensions": [{"bucket":{"field":"attr.latency_ms","edges":[50,200,500,1000]}}],
  "filters": [{"field":"app","op":"eq","value":"tome"}],
  "time_range": {"last":"7d"}
}
// →
{
  "rows": [
    {"attr.latency_ms":"<50","count":820,"p95_latency_ms":41.0},
    {"attr.latency_ms":"50-200","count":540,"p95_latency_ms":180.0}
  ],
  "truncated": false, "elapsed_ms": 14,
  "meta": {"buckets":[{"field":"attr.latency_ms","alias":"attr.latency_ms",
                       "edges":[50,200,500,1000],
                       "labels":["<50","50-200","200-500","500-1000","1000+"]}]}
}
​```
```

(Remove the zero-width space before each ``` when pasting — it is only here to keep this nested fence intact.)

- [ ] **Step 4: Commit**

```bash
git add crates/gauge/src/query_cmd.rs README.md
git commit -m "docs(cli): document numeric query DSL with a worked example" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 6: MCP `query_telemetry` description + render

**Files:**
- Modify: `crates/gauge/src/mcp/server.rs` (tool doc-comment)
- Modify: `crates/gauge/src/mcp/render.rs` (`project_query`)

- [ ] **Step 1: Write a failing render test** in `render.rs` tests — an aggregate/bucket query should NOT suggest the count-only trend action:

```rust
    #[test]
    fn project_query_skips_trend_for_aggregate() {
        use gauge_query::{Field, Measure, QueryRequest, TimeRange};
        let resp = json!({ "rows": [ {"avg_latency_ms": 142.0} ], "truncated": false, "elapsed_ms": 5 });
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
        assert!(o.next_actions.iter().all(|a| a.tool != Some("events_over_time")));
    }
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test -p gauge project_query_skips_trend_for_aggregate`
Expected: FAIL — the trend action is still suggested.

- [ ] **Step 3: Guard the trend suggestion** in `render.rs` `project_query`. Replace the whole `if req.granularity.is_none() && let TimeRange::Last { last } = &req.time_range { ... }` block (the one that pushes the `events_over_time` action) with the version below, which adds an `is_plain` gate so the trend is only suggested for plain count-style queries:

```rust
    let is_plain = req.measures.iter().all(|m| m.numeric_field().is_none())
        && !req.dimensions.iter().any(|d| matches!(d, gauge_query::Dimension::Bucket { .. }));
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
```

Run: `cargo test -p gauge project_query_skips_trend_for_aggregate` → PASS.

- [ ] **Step 4: Extend the `query_telemetry` tool description** in `server.rs`. Replace the doc-comment above `pub async fn query_telemetry` with:

```rust
    /// Run an analytics query over anonymous telemetry events. Measures: count, unique_installs, unique_sessions, plus numeric aggregates over a numeric attr.<key> as single-key objects — {"avg":"attr.latency_ms"}, min, max, p50, p90, p95, p99. Dimensions: app, event_name, app_version, os, arch, attr.<key>, and a numeric bucket {"bucket":{"field":"attr.latency_ms","edges":[50,200,500,1000]}} (rows carry the range label; meta.buckets echoes edges+labels). Filters: eq, neq, in, exists, and numeric gt, gte, lt, lte over a numeric attr.<key>. Non-numeric/null attribute values are excluded, never errored. Time ranges: {"last":"7d"} or RFC3339 from/to. Use get_meta first to discover apps, event names, and numeric_attribute_keys.
```

- [ ] **Step 5: Run, lint, commit**

Run: `cargo test -p gauge && cargo clippy --workspace --all-targets -- -D warnings` → PASS.

```bash
git add crates/gauge/src/mcp/server.rs crates/gauge/src/mcp/render.rs
git commit -m "feat(mcp): document numeric DSL in query_telemetry; render skips trend for aggregates" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 7: MCP `numeric_stats` tool

**Files:**
- Modify: `crates/gauge/src/mcp/tools.rs` (`NumericStatsParams`, `numeric_stats_query`)
- Modify: `crates/gauge/src/mcp/render.rs` (`project_numeric_stats`)
- Modify: `crates/gauge/src/mcp/schemas.rs` (`schema_for`)
- Modify: `crates/gauge/src/mcp/server.rs` (`numeric_stats` tool)

- [ ] **Step 1: Write the failing builder test** in `tools.rs` tests:

```rust
    #[test]
    fn numeric_stats_builds_all_aggregates() {
        let q = numeric_stats_query(&NumericStatsParams {
            period: "7d".into(),
            field: "latency_ms".into(),
            app: Some("tome".into()),
            event_name: None,
        });
        // avg/min/max + four percentiles over attr.latency_ms
        assert_eq!(q.measures.len(), 7);
        assert!(q.measures.iter().any(|m| matches!(m, Measure::P95(f) if f.to_string() == "attr.latency_ms")));
        gauge_query::validate(&q).unwrap();
    }
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test -p gauge numeric_stats_builds_all_aggregates`
Expected: FAIL — `NumericStatsParams` not found.

- [ ] **Step 3: Add params + builder** in `tools.rs`. Add a helper to turn a bare attr key into a `Field`, then the param struct and builder:

```rust
/// A bare attr key (e.g. "latency_ms") → `Field::Attr`. Falls back to `attr.<key>` parsing.
fn attr_field(key: &str) -> Field {
    Field::parse(&format!("attr.{key}")).unwrap_or(Field::Attr(key.to_string()))
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NumericStatsParams {
    /// Relative look-back window, e.g. "24h", "7d", "30d".
    pub period: String,
    /// Numeric attribute key (from get_meta's `apps[].numeric_attribute_keys`), e.g. "latency_ms".
    pub field: String,
    /// Restrict to one app. Omit for all apps.
    pub app: Option<String>,
    /// Restrict to one event name. Omit for all events.
    pub event_name: Option<String>,
}

pub fn numeric_stats_query(p: &NumericStatsParams) -> QueryRequest {
    let f = attr_field(&p.field);
    QueryRequest {
        measures: vec![
            Measure::Avg(f.clone()), Measure::Min(f.clone()), Measure::Max(f.clone()),
            Measure::P50(f.clone()), Measure::P90(f.clone()), Measure::P95(f.clone()), Measure::P99(f),
        ],
        dimensions: vec![],
        filters: base_filters(&p.app, &p.event_name),
        time_range: TimeRange::Last { last: p.period.clone() },
        granularity: None,
        order: vec![],
        limit: None,
    }
}
```

Run: `cargo test -p gauge numeric_stats_builds_all_aggregates` → PASS.

- [ ] **Step 4: Add the projector** in `render.rs`:

```rust
/// `numeric_stats` -> single-row `{ avg_*, min_*, max_*, p50_*..p99_* }`.
pub fn project_numeric_stats(resp: &Value, p: &crate::mcp::tools::NumericStatsParams) -> ToolOutcome {
    let row = rows_of(resp).first().cloned().unwrap_or_else(|| json!({}));
    let g = |k: &str| row.get(k).and_then(Value::as_f64);
    let key = &p.field;
    let num = |o: Option<f64>| o.map(|v| format!("{v:.0}")).unwrap_or_else(|| "n/a".into());
    let scope = match (&p.app, &p.event_name) {
        (Some(a), Some(e)) => format!("{a} · {e}"),
        (Some(a), None) => a.clone(),
        (None, Some(e)) => e.clone(),
        (None, None) => "all apps".to_owned(),
    };
    let summary = format!(
        "{key} ({scope}, {}): avg {}, p95 {}, max {}.",
        p.period, num(g(&format!("avg_{key}"))), num(g(&format!("p95_{key}"))), num(g(&format!("max_{key}")))
    );
    let next_actions = vec![NextAction::call(
        format!("See the {key} distribution as a histogram"),
        "numeric_histogram",
        match &p.app {
            Some(a) => json!({ "period": p.period, "field": key, "app": a, "edges": [50, 200, 500, 1000] }),
            None => json!({ "period": p.period, "field": key, "edges": [50, 200, 500, 1000] }),
        },
    )];
    ToolOutcome { summary, trimmed: row, structured: resp.clone(), next_actions }
}
```

- [ ] **Step 5: Register the output schema.** In `schemas.rs`, add `"numeric_stats"` and `"numeric_histogram"` to the `query_envelope_schema` arm of `schema_for` (both return the query envelope shape):

```rust
        "query_telemetry" | "unique_users" | "top_events" | "events_over_time"
        | "numeric_stats" | "numeric_histogram" => Some(query_envelope_schema()),
```

- [ ] **Step 6: Add the tool** in `server.rs`. Add the import `numeric_stats_query, NumericStatsParams` to the `use crate::mcp::tools::{...}` and `project_numeric_stats` to the `use crate::mcp::render::{...}`. Add the method inside the `#[tool_router] impl GaugeMcp` block:

```rust
    /// Summary statistics (avg/min/max and p50/p90/p95/p99) for a numeric attribute over a period. Field is a numeric attr key from get_meta's numeric_attribute_keys (e.g. "latency_ms").
    #[tool(annotations(title = "Numeric stats", read_only_hint = true, open_world_hint = false))]
    pub async fn numeric_stats(
        &self,
        Parameters(p): Parameters<NumericStatsParams>,
    ) -> Result<CallToolResult, McpError> {
        let req = numeric_stats_query(&p);
        Ok(match self.query_to_value(&req).await {
            Ok(v) => project_numeric_stats(&v, &p).into_result(),
            Err(f) => f.into_result(),
        })
    }
```

- [ ] **Step 7: Run, lint, commit** (the tool-count test still expects 5 here and will be updated in Task 8; temporarily it will list 6 — update the literal `5` in `server.rs` test `tool_list_has_annotations_and_output_schemas` to `6` and add `"numeric_stats"` to the schema-checking name list):

Run: `cargo test -p gauge && cargo clippy --workspace --all-targets -- -D warnings` → PASS.

```bash
git add crates/gauge/src/mcp
git commit -m "feat(mcp): add numeric_stats tool (avg/min/max/percentiles)" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 8: MCP `numeric_histogram` tool

**Files:**
- Modify: `crates/gauge/src/mcp/tools.rs` (`NumericHistogramParams`, `numeric_histogram_query`)
- Modify: `crates/gauge/src/mcp/render.rs` (`project_numeric_histogram`)
- Modify: `crates/gauge/src/mcp/server.rs` (`numeric_histogram` tool + tool-count test 6→7)

- [ ] **Step 1: Write the failing builder test** in `tools.rs` tests:

```rust
    #[test]
    fn numeric_histogram_builds_bucket_dimension() {
        let q = numeric_histogram_query(&NumericHistogramParams {
            period: "7d".into(),
            field: "latency_ms".into(),
            edges: vec![50.0, 200.0, 500.0],
            app: None,
            event_name: None,
        });
        assert!(matches!(&q.dimensions[0], Dimension::Bucket { bucket } if bucket.edges.len() == 3));
        assert!(q.measures.contains(&Measure::Count));
        gauge_query::validate(&q).unwrap();
    }
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test -p gauge numeric_histogram_builds_bucket_dimension`
Expected: FAIL — `NumericHistogramParams` not found.

- [ ] **Step 3: Add params + builder** in `tools.rs` (add `BucketSpec, Dimension` to the `use gauge_query::{...}` import):

```rust
#[derive(Debug, Deserialize, JsonSchema)]
pub struct NumericHistogramParams {
    /// Relative look-back window, e.g. "24h", "7d", "30d".
    pub period: String,
    /// Numeric attribute key (from get_meta's numeric_attribute_keys), e.g. "latency_ms".
    pub field: String,
    /// Strictly-ascending bucket edges, e.g. [50, 200, 500, 1000].
    pub edges: Vec<f64>,
    /// Restrict to one app. Omit for all apps.
    pub app: Option<String>,
    /// Restrict to one event name. Omit for all events.
    pub event_name: Option<String>,
}

pub fn numeric_histogram_query(p: &NumericHistogramParams) -> QueryRequest {
    QueryRequest {
        measures: vec![Measure::Count, Measure::UniqueInstalls],
        dimensions: vec![Dimension::Bucket {
            bucket: BucketSpec { field: attr_field(&p.field), edges: p.edges.clone() },
        }],
        filters: base_filters(&p.app, &p.event_name),
        time_range: TimeRange::Last { last: p.period.clone() },
        granularity: None,
        order: vec![],
        limit: None,
    }
}
```

Run: `cargo test -p gauge numeric_histogram_builds_bucket_dimension` → PASS.

- [ ] **Step 4: Add the projector** in `render.rs`:

```rust
/// `numeric_histogram` -> rows `{ <attr-label>, count, unique_installs }` + meta.buckets.
pub fn project_numeric_histogram(resp: &Value, p: &crate::mcp::tools::NumericHistogramParams) -> ToolOutcome {
    let rows = rows_of(resp);
    let alias = format!("attr.{}", p.field);
    let peak = rows.iter().max_by_key(|r| r.get("count").and_then(Value::as_i64).unwrap_or(0));
    let summary = match peak {
        Some(r) => format!(
            "{} buckets of {} ({}), peak {} in {}.",
            rows.len(), p.field, p.period,
            r.get("count").and_then(Value::as_i64).unwrap_or(0),
            r.get(alias.as_str()).and_then(Value::as_str).unwrap_or("?"),
        ),
        None => format!("0 buckets of {} ({}).", p.field, p.period),
    };
    let next_actions = vec![NextAction::call(
        format!("See avg/min/max/percentiles for {}", p.field),
        "numeric_stats",
        match &p.app {
            Some(a) => json!({ "period": p.period, "field": p.field, "app": a }),
            None => json!({ "period": p.period, "field": p.field }),
        },
    )];
    let trimmed = json!({ "buckets": rows.len(), "rows": rows.iter().take(FENCE_ROW_CAP).cloned().collect::<Vec<_>>() });
    ToolOutcome { summary, trimmed, structured: resp.clone(), next_actions }
}
```

- [ ] **Step 5: Add the tool + update the tool-count test** in `server.rs`. Add `numeric_histogram_query, NumericHistogramParams` to the tools import and `project_numeric_histogram` to the render import. Add the method:

```rust
    /// Histogram of a numeric attribute bucketed by caller-supplied edges, with per-bucket count and unique installs. Field is a numeric attr key (e.g. "latency_ms"); edges are strictly ascending, e.g. [50,200,500,1000].
    #[tool(annotations(title = "Numeric histogram", read_only_hint = true, open_world_hint = false))]
    pub async fn numeric_histogram(
        &self,
        Parameters(p): Parameters<NumericHistogramParams>,
    ) -> Result<CallToolResult, McpError> {
        let req = numeric_histogram_query(&p);
        Ok(match self.query_to_value(&req).await {
            Ok(v) => project_numeric_histogram(&v, &p).into_result(),
            Err(f) => f.into_result(),
        })
    }
```

In the `tool_list_has_annotations_and_output_schemas` test, change `assert_eq!(tools.len(), 6, ...)` to `7` and add `"numeric_histogram"` to the schema-checking name array.

- [ ] **Step 6: Run, lint, commit**

Run: `cargo test -p gauge && cargo clippy --workspace --all-targets -- -D warnings` → PASS.

```bash
git add crates/gauge/src/mcp
git commit -m "feat(mcp): add numeric_histogram tool" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 9: TUI `derive_edges` / `nice_round` helpers

**Files:**
- Modify: `crates/gauge/src/tui/data.rs` (`derive_edges`, `nice_round`)

- [ ] **Step 1: Write the failing test** in `data.rs` (add a `#[cfg(test)] mod tests`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn derive_edges_are_sorted_rounded_and_bounded() {
        let e = derive_edges(0.0, 1180.0);
        assert!(!e.is_empty() && e.len() <= 5);
        assert!(e.windows(2).all(|w| w[0] < w[1]), "edges must be strictly ascending");
        // degenerate range still yields a usable single split
        let d = derive_edges(5.0, 5.0);
        assert_eq!(d.len(), 1);
    }
}
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test -p gauge derive_edges_are_sorted_rounded_and_bounded`
Expected: FAIL — `derive_edges` not found.

- [ ] **Step 3: Implement the helpers** in `data.rs`:

```rust
/// Round x up to a "nice" 1/2/5×10ⁿ step (for readable histogram edges).
fn nice_round(x: f64) -> f64 {
    if x <= 0.0 { return 1.0; }
    let mag = 10f64.powf(x.log10().floor());
    let norm = x / mag; // 1.0..10.0
    let nice = if norm < 1.5 { 1.0 } else if norm < 3.0 { 2.0 } else if norm < 7.0 { 5.0 } else { 10.0 };
    nice * mag
}

/// ~6 nicely-rounded interior edges spanning [min, max] (→ up to 5 edges → 6 buckets).
pub fn derive_edges(min: f64, max: f64) -> Vec<f64> {
    if !min.is_finite() || !max.is_finite() || max <= min {
        return vec![nice_round(min.abs().max(1.0))];
    }
    let step = nice_round((max - min) / 6.0);
    let mut edges = Vec::new();
    let mut e = (min / step).ceil() * step;
    if e <= min { e += step; }
    while e < max && edges.len() < 5 {
        edges.push(e);
        e += step;
    }
    if edges.is_empty() { edges.push((min + max) / 2.0); }
    edges
}
```

- [ ] **Step 4: Run, lint, commit**

Run: `cargo test -p gauge derive_edges_are_sorted_rounded_and_bounded && cargo clippy --workspace --all-targets -- -D warnings` → PASS.

```bash
git add crates/gauge/src/tui/data.rs
git commit -m "feat(tui): derive_edges/nice_round for auto-fit histograms" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 10: TUI numeric aggregate in Explore

**Files:**
- Modify: `crates/gauge/src/tui/app.rs` (`ExploreState`, numeric attr axis, `explore_request`)
- Modify: `crates/gauge/src/tui/data.rs` (`Snapshot` already carries `apps`; expose numeric keys)
- Modify: `crates/gauge/src/tui/ui.rs` (`render_explore` picker line)

- [ ] **Step 1: Write the failing test** in `app.rs` (add a `#[cfg(test)] mod tests`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explore_request_supports_numeric_aggregate() {
        let mut app = App::new();
        app.explore.numeric_attr = Some("latency_ms".to_string());
        app.explore.measure_idx = NUMERIC_MEASURE_BASE; // first numeric measure (avg)
        let req = app.explore_request();
        // an avg over attr.latency_ms is built and validates
        assert!(req.measures.iter().any(|m|
            matches!(m, gauge_query::Measure::Avg(f) if f.to_string() == "attr.latency_ms")));
        gauge_query::validate(&req).unwrap();
    }
}
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test -p gauge explore_request_supports_numeric_aggregate`
Expected: FAIL — `numeric_attr` / `NUMERIC_MEASURE_BASE` not found.

- [ ] **Step 3: Extend the Explore state + measures** in `app.rs`. Add a numeric-measure list and a base index, and a selected numeric attr:

```rust
pub const EXPLORE_MEASURES: &[&str] = &[
    "count", "unique_installs", "unique_sessions", // count-style (index 0..3)
    "avg", "min", "max", "p95",                    // numeric (need a numeric_attr)
];
/// Index in EXPLORE_MEASURES at which numeric (attr-requiring) measures begin.
pub const NUMERIC_MEASURE_BASE: usize = 3;
```

Add to `struct ExploreState`:

```rust
    /// Selected numeric attribute key (from get_meta), required by numeric measures/histogram.
    pub numeric_attr: Option<String>,
```

(`ExploreState` derives `Default`, and `Option` defaults to `None`, so no other change to `Default`.)

- [ ] **Step 4: Rebuild `explore_request`** in `app.rs` to emit a numeric-aggregate measure when a numeric measure is selected:

```rust
    pub fn explore_request(&self) -> gauge_query::QueryRequest {
        let measure = EXPLORE_MEASURES[self.explore.measure_idx];
        let measures: Vec<serde_json::Value> = if self.explore.measure_idx >= NUMERIC_MEASURE_BASE {
            // numeric aggregate: {"avg":"attr.<key>"}; fall back to count if no attr chosen
            match &self.explore.numeric_attr {
                Some(key) => vec![serde_json::json!({ measure: format!("attr.{key}") })],
                None => vec![serde_json::json!("count")],
            }
        } else {
            vec![serde_json::json!(measure)]
        };
        let json = serde_json::json!({
            "measures": measures,
            "dimensions": [EXPLORE_DIMENSIONS[self.explore.dimension_idx]],
            "time_range": {"last": self.window.last()},
            "limit": 50
        });
        serde_json::from_value(json).expect("explore request is always valid")
    }
```

(Note: the explicit `order` is dropped here because a numeric aggregate alias like `avg_latency_ms` would not match the literal measure name; default ordering is fine for Explore.)

Run: `cargo test -p gauge explore_request_supports_numeric_aggregate` → PASS.

- [ ] **Step 5: Let the user cycle the numeric attr.** In `app.rs` `on_key`, add a binding (only on the Explore page) to cycle `numeric_attr` through the union of `numeric_attribute_keys` from the current snapshot. Add a key, e.g. `n`:

```rust
            KeyCode::Char('n') if self.page == Page::Explore => {
                let keys: Vec<String> = self
                    .snapshot
                    .as_ref()
                    .map(|s| {
                        let mut v: Vec<String> = s.apps.iter()
                            .flat_map(|a| a.numeric_attribute_keys.iter().cloned())
                            .collect();
                        v.sort_unstable();
                        v.dedup();
                        v
                    })
                    .unwrap_or_default();
                self.explore.numeric_attr = match (&self.explore.numeric_attr, keys.first()) {
                    (None, Some(first)) => Some(first.clone()),
                    (Some(cur), _) => {
                        let next = keys.iter().position(|k| k == cur).map(|i| (i + 1) % keys.len().max(1));
                        next.and_then(|i| keys.get(i).cloned())
                    }
                    (None, None) => None,
                };
            }
```

- [ ] **Step 6: Show the selection** in `ui.rs` `render_explore` picker line. Replace the `picker` paragraph text with:

```rust
    let picker = Paragraph::new(format!(
        "measure (↑): {}    dimension (↓): {}    attr (n): {}    enter: run",
        EXPLORE_MEASURES[app.explore.measure_idx],
        EXPLORE_DIMENSIONS[app.explore.dimension_idx],
        app.explore.numeric_attr.as_deref().unwrap_or("(none)"),
    ))
    .block(Block::default().borders(Borders::ALL).title("Explore"));
```

- [ ] **Step 7: Run, lint, commit**

Run: `cargo test -p gauge && cargo clippy --workspace --all-targets -- -D warnings` → PASS.

```bash
git add crates/gauge/src/tui/app.rs crates/gauge/src/tui/ui.rs
git commit -m "feat(tui): numeric aggregate measures over a chosen attr in Explore" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 11: TUI histogram mode + min/max probe

**Files:**
- Modify: `crates/gauge/src/tui/data.rs` (`fetch_histogram`)
- Modify: `crates/gauge/src/tui/app.rs` (`ExploreState.histogram`, `histogram_requested`, key `h`)
- Modify: `crates/gauge/src/tui/run.rs` (`Msg::Histogram`, spawn probe+bucket)
- Modify: `crates/gauge/src/tui/ui.rs` (`render_explore` histogram branch)

- [ ] **Step 1: Write the failing test** for the histogram fetch's query shapes in `data.rs` tests (we test the request builders, not the network):

```rust
    #[test]
    fn histogram_probe_then_bucket_request_shapes() {
        // the probe asks for min+max of the attr
        let probe = histogram_probe_request(TimeWindow::D7, "latency_ms");
        assert!(probe.measures.iter().any(|m| matches!(m, Measure::Min(_))));
        assert!(probe.measures.iter().any(|m| matches!(m, Measure::Max(_))));
        // the bucket request uses derived edges as a Dimension::Bucket
        let bucket = histogram_bucket_request(TimeWindow::D7, "latency_ms", vec![50.0, 200.0]);
        assert!(matches!(&bucket.dimensions[0], gauge_query::Dimension::Bucket { .. }));
    }
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test -p gauge histogram_probe_then_bucket_request_shapes`
Expected: FAIL — request builders not found.

- [ ] **Step 3: Add the request builders + async fetch** in `data.rs` (add `BucketSpec, Dimension` to the `use gauge_query::{...}` import):

```rust
fn numeric_attr_field(key: &str) -> Field {
    Field::parse(&format!("attr.{key}")).unwrap_or(Field::Attr(key.to_string()))
}

pub fn histogram_probe_request(w: TimeWindow, key: &str) -> QueryRequest {
    let f = numeric_attr_field(key);
    QueryRequest { measures: vec![Measure::Min(f.clone()), Measure::Max(f)], ..base(w) }
}

pub fn histogram_bucket_request(w: TimeWindow, key: &str, edges: Vec<f64>) -> QueryRequest {
    QueryRequest {
        measures: vec![Measure::Count],
        dimensions: vec![Dimension::Bucket {
            bucket: BucketSpec { field: numeric_attr_field(key), edges },
        }],
        ..base(w)
    }
}

/// Probe min/max, derive edges, then fetch the bucketed counts.
pub async fn fetch_histogram(
    api: &ApiClient, w: TimeWindow, key: &str,
) -> Result<QueryResponse, ClientError> {
    let probe = api.query(&histogram_probe_request(w, key)).await?;
    let row = probe.rows.first().cloned().unwrap_or_default();
    let g = |k: &str| row.get(k).and_then(serde_json::Value::as_f64);
    let (min, max) = (
        g(&format!("min_{key}")).unwrap_or(0.0),
        g(&format!("max_{key}")).unwrap_or(0.0),
    );
    let edges = derive_edges(min, max);
    api.query(&histogram_bucket_request(w, key, edges)).await
}
```

(Add `use gauge_query::QueryResponse;` if not already imported — `app.rs` imports it, `data.rs` needs it here.)

Run: `cargo test -p gauge histogram_probe_then_bucket_request_shapes` → PASS.

- [ ] **Step 4: Add histogram state + trigger** in `app.rs`. Add to `struct ExploreState`:

```rust
    pub histogram_requested: bool,
    pub histogram: Option<gauge_query::QueryResponse>,
```

Add a key binding in `on_key` (Explore page) to request a histogram for the selected numeric attr:

```rust
            KeyCode::Char('h') if self.page == Page::Explore && self.explore.numeric_attr.is_some() => {
                self.explore.histogram_requested = true;
            }
```

- [ ] **Step 5: Wire the async probe+bucket** in `run.rs`. Add a `Msg` variant:

```rust
    Histogram(Result<gauge_query::QueryResponse, String>),
```

In `event_loop`, after the `if app.explore.run_requested { ... }` block, add:

```rust
        if app.explore.histogram_requested {
            app.explore.histogram_requested = false;
            if let Some(key) = app.explore.numeric_attr.clone() {
                let api = api.clone();
                let tx = tx.clone();
                let w = app.window;
                tokio::spawn(async move {
                    let result = crate::tui::data::fetch_histogram(&api, w, &key).await.map_err(|e| e.to_string());
                    let _ = tx.send(Msg::Histogram(result)).await;
                });
            }
        }
```

Add to the `match msg { ... }` arms:

```rust
                Msg::Histogram(Ok(r)) => app.explore.histogram = Some(r),
                Msg::Histogram(Err(e)) => app.stale = Some(e),
```

- [ ] **Step 6: Render the histogram** in `ui.rs` `render_explore`. When `app.explore.histogram` is `Some`, draw a horizontal bar chart instead of the JSON lines. Replace the `match &app.explore.result { ... }` block with:

```rust
    if let Some(hist) = &app.explore.histogram {
        let attr_alias = app
            .explore
            .numeric_attr
            .as_ref()
            .map(|k| format!("attr.{k}"))
            .unwrap_or_default();
        let bars: Vec<Bar> = hist
            .rows
            .iter()
            .map(|r| {
                Bar::default()
                    .label(r[attr_alias.as_str()].as_str().unwrap_or("?").to_string().into())
                    .value(r["count"].as_i64().unwrap_or(0) as u64)
            })
            .collect();
        let chart = BarChart::default()
            .block(Block::default().borders(Borders::ALL).title("Histogram (h to refresh)"))
            .direction(Direction::Horizontal)
            .bar_width(1)
            .data(BarGroup::default().bars(&bars));
        f.render_widget(chart, chunks[1]);
        return;
    }
    let block = Block::default().borders(Borders::ALL).title("Result");
    match &app.explore.result {
        None => f.render_widget(Paragraph::new("press enter to run · n: pick attr · h: histogram").block(block), chunks[1]),
        Some(resp) => {
            let lines: Vec<Line> = resp.rows.iter()
                .map(|r| Line::from(serde_json::to_string(r).unwrap_or_default()))
                .collect();
            f.render_widget(Paragraph::new(lines).block(block), chunks[1]);
        }
    }
```

- [ ] **Step 7: Run, lint, commit**

Run: `cargo test -p gauge && cargo clippy --workspace --all-targets -- -D warnings` → PASS.

```bash
git add crates/gauge/src/tui
git commit -m "feat(tui): auto-fit histogram view with min/max probe in Explore" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

# Phase 3 — verification

### Task 12: Full verification + docs sync

**Files:**
- Modify: `docs/superpowers/specs/2026-06-17-numeric-query-dsl-design.md` (tick acceptance criteria if you keep a checklist; otherwise no-op)
- Modify: this plan (tick all checkboxes)

- [ ] **Step 1: Full workspace test + lint**

Run: `cargo test --workspace`
Run: `cargo clippy --workspace --all-targets -- -D warnings`
Run: `cargo fmt --all --check`
Expected: all PASS / no diffs. Fix any failures before proceeding.

- [ ] **Step 2: Confirm the snapshots are committed and accurate**

Run: `git status --porcelain crates/gauge-server/src/snapshots`
Expected: empty (all `.snap` files committed; no stray `.snap.new`).

- [ ] **Step 3: Spec acceptance-criteria walk-through.** Re-read §9 of the spec and confirm each item maps to a task: bucket (T3), aggregates incl. percentiles (T1), numeric filters (T2), graceful exclusion (T1/T2/T3 via the shared cast + IS NOT NULL), no value leakage (T2/T3 no-leak asserts), snapshots (T1/T2/T3), CLI docs (T5), MCP description + two tools + render (T6/T7/T8), TUI numeric + histogram (T9/T10/T11), `numeric_attribute_keys` (T4). Note any gap and add a follow-up task.

- [ ] **Step 4: Optional fact-check of the README example**

Run the `/midnight-fact-check:fast-check` skill against the README Query DSL section, or manually confirm the example response shape matches `QueryResponse` + `BucketMeta`. (Skip if unavailable.)

- [ ] **Step 5: Tick this plan's checkboxes and commit the docs sync**

```bash
git add docs/superpowers
git commit -m "docs(plan): mark numeric query DSL plan complete" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

- [ ] **Step 6: Finish the branch.** Use the `superpowers:finishing-a-development-branch` skill to choose how to integrate (open a PR referencing issue #22, merge, or keep iterating).

---

## Self-review notes (author)

- **Spec coverage:** every §9 acceptance criterion has a task (see Task 12 Step 3 mapping). The cross-client scope (CLI/MCP/TUI) and `numeric_attribute_keys` are all covered.
- **Type consistency:** `Measure::alias()` returns `String` everywhere (Task 1 changes its return type; the only caller, `top_events_query`, is updated in the same task). `Dimension::Field`/`Dimension::Bucket { bucket }` naming is consistent across `request.rs`, `sqlbuild.rs`, `tools.rs`, `data.rs`, and `render.rs`. Decode helpers `float_value`/`bucket_value` and `bucket_labels` are referenced with the same signatures where defined.
- **Compile-green ordering:** Task 1 changes `Measure` and fixes its only non-`Copy` caller in the same task; Task 3 changes `dimensions` type and fixes every construction site (`tui/data.rs`, `mcp/tools.rs`) in the same task; the MCP tool-count literal is bumped 5→6 (Task 7) then 6→7 (Task 8) as each tool lands.
