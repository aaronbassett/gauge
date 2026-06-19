# TUI Dashboard Redesign — Plan 2: Panels & Data Layer

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the panel framework — a `Panel` trait, a `build(spec)` factory, and seven panel kinds (`timeseries`, `stat`, `top_n`, `breakdown`, `numeric_stats`, `histogram`, `apps_table`) — plus a concurrent, deduplicated data layer. Each panel's query-building is validated against `gauge_query::validate`; rendering is smoke-tested against a `TestBackend` buffer.

**Architecture:** A new `tui/panels/` module tree. Each panel is a small **stateless** struct built from a `PanelSpec` (Plan 1). It knows (a) the `QueryRequest`s it needs given a `PanelCtx`, and (b) how to draw its results with a `Theme`. **Every request is deterministic** (no wall-clock in the request), so a panel can recompute its request keys at render time and look up exactly its own results in the shared `ResultMap` — no per-panel state, no cross-panel bleed. The data layer (`data.rs`, extended additively) collects every visible panel's requests, dedupes by serialized request, and fetches them concurrently. Nothing is wired into the run loop yet — `gauge tui` still runs the old UI until Plan 3. All items are `pub`, so no dead-code warnings.

**Tech Stack:** Rust 2024, `ratatui 0.29` (`Frame`, `Block`, `Chart`, `BarChart`, `Table`, `TestBackend`), `gauge-query` (`QueryRequest`/`Measure`/`Dimension`/`Field`/`Filter`/`Order`/`TimeRange`/`validate`), `futures` (`join_all`), `serde_json`.

**Plan series:** Plan 2 of 5. Depends on Plan 1 (theme, config, layout). Does **not** modify `app.rs`/`run.rs`/`ui.rs`.

**Key design rule — deterministic requests & render lookup:** A panel's `render` recomputes `self.data_requests(ctx)` (cheap) to get its request key(s), then looks each result up in the full `ResultMap` by key. Therefore **no request may depend on `Instant::now`/`OffsetDateTime::now`** — for period-over-period deltas, `stat` fetches a *doubled* window as a timeseries and splits it at the midpoint, rather than computing an absolute "previous" range from the current time.

---

## File Structure

| File | Responsibility |
|---|---|
| `crates/gauge/src/tui/config.rs` | **Modify.** Add `edges: Vec<f64>` to `PanelSpec`. |
| `crates/gauge/src/tui/data.rs` | **Modify.** Add `TimeWindow::doubled_last`; `QuerySource` trait; `collect_requests`; `fetch_all`. (Keep existing `Snapshot`/`fetch`/histogram helpers for the old UI.) |
| `crates/gauge/src/tui/panels/mod.rs` | **Create.** `Panel` trait, `PanelCtx`, `LabeledRequest`/`RequestKey`/`ResultMap`, helpers (`merge_filters`, `base_request`, `count_measure`, `agg_measure`, `resolve_numeric_attr`, `desc`, `nth_response`, `panel_block`, `accent`, `lerp_color`), `build`, test helpers. |
| `crates/gauge/src/tui/panels/{timeseries,stat,top_n,breakdown,numeric_stats,histogram,apps_table}.rs` | **Create.** The seven panel kinds. |
| `crates/gauge/src/tui/mod.rs` | **Modify.** Register `pub mod panels;`. |

---

## Task 1: Add histogram `edges` to `PanelSpec`

**Files:**
- Modify: `crates/gauge/src/tui/config.rs`

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `config.rs`:

```rust
    #[test]
    fn panel_spec_parses_histogram_edges() {
        let toml = r#"
active_preset = "d"
[[preset]]
name = "d"
  [[preset.panel]]
  kind = "histogram"
  attr = "latency_ms"
  edges = [50, 200, 600]
"#;
        let cfg: DashboardConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.presets[0].panels[0].edges, vec![50.0, 200.0, 600.0]);
    }

    #[test]
    fn panel_spec_edges_default_empty() {
        let cfg = default_builtin();
        assert!(cfg.presets[0].panels[0].edges.is_empty());
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p gauge-client tui::config::tests::panel_spec_parses_histogram_edges -- --nocapture`
Expected: FAIL to compile — no field `edges`.

- [ ] **Step 3: Add the field**

In `PanelSpec` (after `attr`, before `filters`):

```rust
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<f64>, // histogram bucket edges
```

Add `edges: vec![],` to every `PanelSpec { .. }` literal in `default_builtin()` (the `stat`/`breakdown` helpers and the `timeseries`/`top_events`/`latency` literals), alongside `filters: vec![],`.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p gauge-client tui::config -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/gauge/src/tui/config.rs
git commit -m "feat(tui): add histogram edges to PanelSpec"
```

---

## Task 2: `TimeWindow::doubled_last`

**Files:**
- Modify: `crates/gauge/src/tui/data.rs`

`stat` needs a window twice as long as the current one (to split into current/previous halves).

- [ ] **Step 1: Write the failing test**

Add to the existing `#[cfg(test)] mod tests` in `data.rs`:

```rust
    #[test]
    fn doubled_last_doubles_each_window() {
        assert_eq!(TimeWindow::H1.doubled_last(), "2h");
        assert_eq!(TimeWindow::H24.doubled_last(), "48h");
        assert_eq!(TimeWindow::D7.doubled_last(), "14d");
        assert_eq!(TimeWindow::D30.doubled_last(), "60d");
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p gauge-client tui::data::tests::doubled_last_doubles_each_window -- --nocapture`
Expected: FAIL to compile — no method `doubled_last`.

- [ ] **Step 3: Implement**

Add to `impl TimeWindow` in `data.rs`:

```rust
    /// The relative range string for twice this window (for current-vs-previous deltas).
    pub fn doubled_last(&self) -> &'static str {
        match self {
            Self::H1 => "2h",
            Self::H24 => "48h",
            Self::D7 => "14d",
            Self::D30 => "60d",
        }
    }
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p gauge-client tui::data -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/gauge/src/tui/data.rs
git commit -m "feat(tui): TimeWindow::doubled_last for stat deltas"
```

---

## Task 3: Panel trait, context, and shared helpers

**Files:**
- Create: `crates/gauge/src/tui/panels/mod.rs`
- Modify: `crates/gauge/src/tui/mod.rs`

- [ ] **Step 1: Create the module and register it**

Create `crates/gauge/src/tui/panels/mod.rs`:

```rust
//! The panel framework: one small, stateless unit per dashboard panel kind. Each panel
//! turns a `PanelCtx` (window + global filters + meta) into the `QueryRequest`s it needs,
//! and renders its results with the active `Theme`. Requests are deterministic, so a
//! panel recomputes its keys at render time and looks up exactly its own results.

pub mod apps_table;
pub mod breakdown;
pub mod histogram;
pub mod numeric_stats;
pub mod stat;
pub mod timeseries;
pub mod top_n;

use std::collections::BTreeMap;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders};

use gauge_query::{AppMeta, Dir, Field, Filter, Measure, Order, QueryRequest, QueryResponse, TimeRange};

use crate::tui::config::PanelSpec;
use crate::tui::data::TimeWindow;
use crate::tui::theme::Theme;

/// Everything a panel needs to build its queries.
pub struct PanelCtx<'a> {
    pub window: TimeWindow,
    pub filters: &'a [Filter],
    pub meta: &'a [AppMeta],
}

/// A stable dedup key for a query (its canonical JSON serialization).
pub type RequestKey = String;

/// A query plus its dedup key.
#[derive(Debug, Clone)]
pub struct LabeledRequest {
    pub key: RequestKey,
    pub request: QueryRequest,
}

impl LabeledRequest {
    pub fn new(request: QueryRequest) -> Self {
        let key = serde_json::to_string(&request).unwrap_or_default();
        Self { key, request }
    }
}

/// Fetched results keyed by `RequestKey`.
pub type ResultMap = BTreeMap<RequestKey, Result<QueryResponse, String>>;

/// One dashboard panel.
pub trait Panel {
    fn title(&self) -> String;
    /// Queries this panel needs (deterministic). May be empty.
    fn data_requests(&self, ctx: &PanelCtx) -> Vec<LabeledRequest>;
    /// Draw the panel. `ctx` lets the panel recompute its request keys to find results.
    fn render(&self, f: &mut Frame, area: Rect, ctx: &PanelCtx, results: &ResultMap, theme: &Theme);
}

// ---- shared helpers ----

/// Global filters followed by a panel's static pins.
pub fn merge_filters(global: &[Filter], pins: &[Filter]) -> Vec<Filter> {
    let mut v = global.to_vec();
    v.extend(pins.iter().cloned());
    v
}

/// A `count`-style measure name → `Measure`.
pub fn count_measure(name: &str) -> Option<Measure> {
    match name {
        "events" | "count" => Some(Measure::Count),
        "unique_installs" => Some(Measure::UniqueInstalls),
        "unique_sessions" => Some(Measure::UniqueSessions),
        _ => None,
    }
}

/// An aggregate measure name over a numeric attr field.
pub fn agg_measure(name: &str, field: Field) -> Option<Measure> {
    Some(match name {
        "avg" => Measure::Avg(field),
        "min" => Measure::Min(field),
        "max" => Measure::Max(field),
        "p50" => Measure::P50(field),
        "p90" => Measure::P90(field),
        "p95" => Measure::P95(field),
        "p99" => Measure::P99(field),
        _ => return None,
    })
}

/// The numeric attr to use: explicit, else the first numeric attr in meta (sorted), else None.
pub fn resolve_numeric_attr(explicit: &Option<String>, meta: &[AppMeta]) -> Option<String> {
    if let Some(a) = explicit {
        return Some(a.clone());
    }
    let mut keys: Vec<String> = meta
        .iter()
        .flat_map(|a| a.numeric_attribute_keys.iter().cloned())
        .collect();
    keys.sort_unstable();
    keys.dedup();
    keys.into_iter().next()
}

/// A base request for the current window with global + pinned filters applied.
pub fn base_request(ctx: &PanelCtx, pins: &[Filter]) -> QueryRequest {
    QueryRequest {
        measures: vec![Measure::Count],
        dimensions: vec![],
        filters: merge_filters(ctx.filters, pins),
        time_range: TimeRange::Last {
            last: ctx.window.last().to_string(),
        },
        granularity: None,
        order: vec![],
        limit: None,
    }
}

/// Order descending by an output alias.
pub fn desc(alias: &str) -> Order {
    Order { field: alias.to_string(), dir: Dir::Desc }
}

/// Recompute a panel's i-th request key and fetch its OK response from the full map.
pub fn nth_response<'r>(
    reqs: &[LabeledRequest],
    i: usize,
    results: &'r ResultMap,
) -> Option<&'r QueryResponse> {
    reqs.get(i)
        .and_then(|lr| results.get(&lr.key))
        .and_then(|r| r.as_ref().ok())
}

/// A themed bordered block with a bold title.
pub fn panel_block(title: &str, theme: &Theme) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(theme.border.border_type())
        .border_style(Style::default().fg(theme.palette.surface))
        .title(Span::styled(
            format!(" {title} "),
            Style::default().fg(theme.palette.text).add_modifier(Modifier::BOLD),
        ))
}

/// Series/accent colour for index `i`.
pub fn accent(theme: &Theme, i: usize) -> Color {
    let a = &theme.palette.accents;
    a[i % a.len().max(1)]
}

/// Linear-interpolate two RGB colours; non-RGB colours fall back to `a`.
pub fn lerp_color(a: Color, b: Color, t: f64) -> Color {
    match (a, b) {
        (Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) => {
            let m = |x: u8, y: u8| (x as f64 + (y as f64 - x as f64) * t.clamp(0.0, 1.0)).round() as u8;
            Color::Rgb(m(ar, br), m(ag, bg), m(ab, bb))
        }
        _ => a,
    }
}

/// A compact inline sparkline using block-eighths.
pub fn braille_sparkline(values: &[f64]) -> String {
    const TICKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let max = values.iter().cloned().fold(0.0_f64, f64::max).max(1.0);
    values
        .iter()
        .map(|v| {
            let idx = ((v / max) * (TICKS.len() as f64 - 1.0)).round().clamp(0.0, 7.0) as usize;
            TICKS[idx]
        })
        .collect()
}

/// Build a panel from its spec, or an error naming the bad kind.
pub fn build(spec: &PanelSpec) -> Result<Box<dyn Panel>, String> {
    Ok(match spec.kind.as_str() {
        "timeseries" => Box::new(timeseries::Timeseries::from_spec(spec)?),
        "stat" => Box::new(stat::Stat::from_spec(spec)?),
        "top_n" => Box::new(top_n::TopN::from_spec(spec)?),
        "breakdown" => Box::new(breakdown::Breakdown::from_spec(spec)?),
        "numeric_stats" => Box::new(numeric_stats::NumericStats::from_spec(spec)?),
        "histogram" => Box::new(histogram::Histogram::from_spec(spec)?),
        "apps_table" => Box::new(apps_table::AppsTable::from_spec(spec)?),
        other => return Err(format!("unknown panel kind `{other}`")),
    })
}

#[cfg(test)]
pub(crate) fn buffer_text(buf: &ratatui::buffer::Buffer) -> String {
    let area = buf.area;
    let mut s = String::new();
    for y in 0..area.height {
        for x in 0..area.width {
            s.push_str(buf[(x, y)].symbol());
        }
    }
    s
}

#[cfg(test)]
pub(crate) fn render_panel(
    panel: &dyn Panel,
    ctx: &PanelCtx,
    results: &ResultMap,
    theme: &Theme,
) -> String {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    let mut term = Terminal::new(TestBackend::new(60, 12)).unwrap();
    term.draw(|f| panel.render(f, f.area(), ctx, results, theme)).unwrap();
    buffer_text(term.backend().buffer())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gauge_query::FilterOp;

    fn pin(field: &str) -> Filter {
        Filter { field: Field::parse(field).unwrap(), op: FilterOp::Exists, value: None }
    }

    #[test]
    fn merge_filters_keeps_global_then_pins() {
        let merged = merge_filters(&[pin("app")], &[pin("os")]);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].field, Field::App);
        assert_eq!(merged[1].field, Field::Os);
    }

    #[test]
    fn count_and_agg_measures_resolve() {
        assert_eq!(count_measure("events"), Some(Measure::Count));
        assert!(count_measure("nope").is_none());
        assert!(matches!(agg_measure("p95", Field::Attr("l".into())).unwrap(), Measure::P95(_)));
        assert!(agg_measure("median", Field::Attr("x".into())).is_none());
    }

    #[test]
    fn resolve_numeric_attr_prefers_explicit_then_meta() {
        let meta = vec![AppMeta {
            app: "a".into(),
            event_names: vec![],
            attribute_keys: vec![],
            numeric_attribute_keys: vec!["latency_ms".into(), "bytes".into()],
            first_event: None,
            last_event: None,
            total_events: 0,
        }];
        assert_eq!(resolve_numeric_attr(&Some("x".into()), &meta), Some("x".into()));
        assert_eq!(resolve_numeric_attr(&None, &meta), Some("bytes".into())); // sorted-first
        assert_eq!(resolve_numeric_attr(&None, &[]), None);
    }
}
```

Add `pub mod panels;` to `crates/gauge/src/tui/mod.rs` (full file becomes `pub mod app; pub mod config; pub mod data; pub mod layout; pub mod panels; pub mod run; pub mod theme; pub mod ui;`).

- [ ] **Step 2: Add temporary panel stubs so the crate compiles**

The `mod` lines + `build()` reference seven panel modules. Create each as a stub so Task 3 compiles; Tasks 4–10 replace them. For each name, create `crates/gauge/src/tui/panels/<name>.rs`. The stub must implement `Panel` so `build()` type-checks. Example `timeseries.rs`:

```rust
use ratatui::Frame;
use ratatui::layout::Rect;

use crate::tui::config::PanelSpec;
use crate::tui::panels::{LabeledRequest, Panel, PanelCtx, ResultMap};
use crate::tui::theme::Theme;

pub struct Timeseries;

impl Timeseries {
    pub fn from_spec(_spec: &PanelSpec) -> Result<Self, String> {
        Err("timeseries not implemented yet".into())
    }
}

impl Panel for Timeseries {
    fn title(&self) -> String {
        "timeseries".into()
    }
    fn data_requests(&self, _ctx: &PanelCtx) -> Vec<LabeledRequest> {
        vec![]
    }
    fn render(&self, _f: &mut Frame, _a: Rect, _c: &PanelCtx, _r: &ResultMap, _t: &Theme) {}
}
```

Create the analogous stub for `Stat`, `TopN`, `Breakdown`, `NumericStats`, `Histogram`, `AppsTable` (same shape, different struct name and module doc). These compile and let `build()` type-check (each `from_spec` returns `Err`, so `build` returns `Err` for now, which is fine).

- [ ] **Step 3: Run the tests**

Run: `cargo test -p gauge-client tui::panels::tests -- --nocapture`
Expected: PASS (`merge_filters_keeps_global_then_pins`, `count_and_agg_measures_resolve`, `resolve_numeric_attr_prefers_explicit_then_meta`).

- [ ] **Step 4: Commit**

```bash
git add crates/gauge/src/tui/panels crates/gauge/src/tui/mod.rs
git commit -m "feat(tui): panel trait, context, and shared helpers (stubs)"
```

---

## Task 4: `timeseries` panel

**Files:**
- Modify: `crates/gauge/src/tui/panels/timeseries.rs`

- [ ] **Step 1: Replace the stub**

Replace `crates/gauge/src/tui/panels/timeseries.rs` with:

```rust
//! Multi-series line chart over time. Without `group_by`, each configured metric is a
//! series. With `group_by`, the first metric is split into one series per dimension value.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::symbols;
use ratatui::text::Span;
use ratatui::widgets::{Axis, Chart, Dataset, GraphType, Paragraph};

use gauge_query::{Dimension, Field, Measure, QueryRequest};

use crate::tui::config::PanelSpec;
use crate::tui::panels::{
    LabeledRequest, Panel, PanelCtx, ResultMap, accent, base_request, count_measure, nth_response,
    panel_block,
};
use crate::tui::theme::Theme;

pub struct Timeseries {
    title: String,
    metrics: Vec<(String, Measure)>,
    group_by: Option<Field>,
    pins: Vec<gauge_query::Filter>,
}

impl Timeseries {
    pub fn from_spec(spec: &PanelSpec) -> Result<Self, String> {
        let names = if spec.metrics.is_empty() {
            vec!["events".to_string()]
        } else {
            spec.metrics.clone()
        };
        let mut metrics = Vec::new();
        for n in names {
            let m = count_measure(&n).ok_or_else(|| format!("timeseries: bad metric `{n}`"))?;
            metrics.push((n, m));
        }
        let group_by = match &spec.group_by {
            Some(g) => Some(Field::parse(g).map_err(|e| e.to_string())?),
            None => None,
        };
        Ok(Self {
            title: spec.title.clone().unwrap_or_else(|| "Activity".into()),
            metrics,
            group_by,
            pins: spec.filters.clone(),
        })
    }

    fn request(&self, ctx: &PanelCtx) -> QueryRequest {
        let mut req = base_request(ctx, &self.pins);
        req.granularity = Some(ctx.window.granularity());
        match &self.group_by {
            Some(field) => {
                req.measures = vec![self.metrics[0].1.clone()];
                req.dimensions = vec![Dimension::Field(field.clone())];
            }
            None => req.measures = self.metrics.iter().map(|(_, m)| m.clone()).collect(),
        }
        req
    }
}

impl Panel for Timeseries {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn data_requests(&self, ctx: &PanelCtx) -> Vec<LabeledRequest> {
        vec![LabeledRequest::new(self.request(ctx))]
    }

    fn render(&self, f: &mut Frame, area: Rect, ctx: &PanelCtx, results: &ResultMap, theme: &Theme) {
        let block = panel_block(&self.title, theme);
        let reqs = self.data_requests(ctx);
        let Some(resp) = nth_response(&reqs, 0, results) else {
            f.render_widget(
                Paragraph::new("loading…").block(block).style(Style::default().fg(theme.palette.text)),
                area,
            );
            return;
        };

        let mut buckets: Vec<String> = resp
            .rows
            .iter()
            .filter_map(|r| r["time_bucket"].as_str().map(str::to_string))
            .collect();
        buckets.sort();
        buckets.dedup();

        let mut series: std::collections::BTreeMap<String, Vec<(f64, f64)>> = Default::default();
        let mut y_max = 1.0f64;
        for row in &resp.rows {
            let Some(bucket) = row["time_bucket"].as_str() else { continue };
            let x = buckets.iter().position(|b| b == bucket).unwrap_or(0) as f64;
            match &self.group_by {
                Some(field) => {
                    let name = row[field.to_string().as_str()].as_str().unwrap_or("?").to_string();
                    let v = row["count"].as_f64().unwrap_or(0.0);
                    y_max = y_max.max(v);
                    series.entry(name).or_default().push((x, v));
                }
                None => {
                    for (label, measure) in &self.metrics {
                        let v = row[measure.alias().as_str()].as_f64().unwrap_or(0.0);
                        y_max = y_max.max(v);
                        series.entry(label.clone()).or_default().push((x, v));
                    }
                }
            }
        }

        let datasets: Vec<Dataset> = series
            .iter()
            .enumerate()
            .map(|(i, (name, points))| {
                Dataset::default()
                    .name(name.clone())
                    .marker(symbols::Marker::Braille)
                    .graph_type(GraphType::Line)
                    .style(Style::default().fg(accent(theme, i)))
                    .data(points)
            })
            .collect();

        let x_max = buckets.len().saturating_sub(1).max(1) as f64;
        let chart = Chart::new(datasets)
            .block(block)
            .x_axis(Axis::default().bounds([0.0, x_max]))
            .y_axis(
                Axis::default()
                    .bounds([0.0, y_max * 1.1])
                    .labels(vec![Span::raw("0"), Span::raw(format!("{}", y_max as i64))]),
            );
        f.render_widget(chart, area);
    }
}
```

- [ ] **Step 2: Write the failing test**

Append to `crates/gauge/src/tui/panels/timeseries.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::config::DashboardConfig;
    use crate::tui::data::TimeWindow;
    use crate::tui::panels::{ResultMap, render_panel};

    fn spec() -> PanelSpec {
        DashboardConfig::default_builtin().presets.remove(0).panels.remove(0)
    }

    #[test]
    fn builds_a_valid_granular_request() {
        let p = Timeseries::from_spec(&spec()).unwrap();
        let ctx = PanelCtx { window: TimeWindow::D7, filters: &[], meta: &[] };
        let reqs = p.data_requests(&ctx);
        assert_eq!(reqs.len(), 1);
        assert!(reqs[0].request.granularity.is_some());
        assert_eq!(reqs[0].request.measures.len(), 3);
        gauge_query::validate(&reqs[0].request).unwrap();
    }

    #[test]
    fn renders_loading_without_data() {
        let p = Timeseries::from_spec(&spec()).unwrap();
        let theme = DashboardConfig::default_builtin().resolve_theme();
        let ctx = PanelCtx { window: TimeWindow::D7, filters: &[], meta: &[] };
        assert!(render_panel(&p, &ctx, &ResultMap::new(), &theme).contains("loading"));
    }
}
```

- [ ] **Step 3: Run** — `cargo test -p gauge-client tui::panels::timeseries -- --nocapture` → PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/gauge/src/tui/panels/timeseries.rs
git commit -m "feat(tui): timeseries panel"
```

---

## Task 5: `stat` panel (number + Δ + sparkline, deterministic)

**Files:**
- Modify: `crates/gauge/src/tui/panels/stat.rs`

A `stat` shows one scalar metric. For **count-style** metrics it fetches a *doubled* window as a granular timeseries (one deterministic request) and splits the buckets at the midpoint: recent half → current value + sparkline; older half → previous value for the ▲▼ delta. For **aggregate** metrics (avg/p95/…) it fetches a single current-window aggregate (no delta — percentiles don't sum across buckets).

> **Known approximation:** the current/previous split is by bucket *index* (`len/2`), not an exact timestamp boundary. With dense buckets this matches the window split; with sparse data it's approximate — acceptable for a trend arrow.

- [ ] **Step 1: Replace the stub**

Replace `crates/gauge/src/tui/panels/stat.rs` with:

```rust
//! A single scalar tile: big number + Δ vs previous period + sparkline.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use gauge_query::{Field, Measure, QueryRequest, TimeRange};

use crate::tui::config::PanelSpec;
use crate::tui::panels::{
    LabeledRequest, Panel, PanelCtx, ResultMap, accent, agg_measure, base_request, braille_sparkline,
    count_measure, nth_response, panel_block, resolve_numeric_attr,
};
use crate::tui::theme::Theme;

pub struct Stat {
    title: String,
    metric: String,
    explicit_attr: Option<String>,
    pins: Vec<gauge_query::Filter>,
}

impl Stat {
    pub fn from_spec(spec: &PanelSpec) -> Result<Self, String> {
        let metric = spec.metric.clone().ok_or_else(|| "stat: `metric` is required".to_string())?;
        Ok(Self {
            title: spec.title.clone().unwrap_or_else(|| metric.clone()),
            metric,
            explicit_attr: spec.attr.clone(),
            pins: spec.filters.clone(),
        })
    }

    fn is_count(&self) -> bool {
        count_measure(&self.metric).is_some()
    }

    fn measure(&self, ctx: &PanelCtx) -> Option<Measure> {
        if let Some(m) = count_measure(&self.metric) {
            return Some(m);
        }
        let attr = resolve_numeric_attr(&self.explicit_attr, ctx.meta)?;
        agg_measure(&self.metric, Field::Attr(attr))
    }

    fn request(&self, ctx: &PanelCtx) -> Option<QueryRequest> {
        let m = self.measure(ctx)?;
        let mut r = base_request(ctx, &self.pins);
        r.measures = vec![m];
        if self.is_count() {
            // Doubled window, granular → split at the midpoint for current vs previous.
            r.time_range = TimeRange::Last { last: ctx.window.doubled_last().to_string() };
            r.granularity = Some(ctx.window.granularity());
        }
        Some(r)
    }
}

impl Panel for Stat {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn data_requests(&self, ctx: &PanelCtx) -> Vec<LabeledRequest> {
        match self.request(ctx) {
            Some(req) => vec![LabeledRequest::new(req)],
            None => vec![],
        }
    }

    fn render(&self, f: &mut Frame, area: Rect, ctx: &PanelCtx, results: &ResultMap, theme: &Theme) {
        let block = panel_block(&self.title, theme);
        let inner = block.inner(area);
        f.render_widget(block, area);

        let reqs = self.data_requests(ctx);
        let resp = nth_response(&reqs, 0, results);
        let Some(resp) = resp else {
            let msg = if reqs.is_empty() { "no numeric attributes yet" } else { "loading…" };
            f.render_widget(
                Paragraph::new(msg).style(Style::default().fg(theme.palette.muted)),
                inner,
            );
            return;
        };

        let (value, prev, sparks): (Option<f64>, Option<f64>, Vec<f64>) = if self.is_count() {
            // Sort buckets, read the measure column per bucket, split at the midpoint.
            let alias = self.measure(ctx).map(|m| m.alias()).unwrap_or_else(|| "count".into());
            let mut pairs: Vec<(String, f64)> = resp
                .rows
                .iter()
                .filter_map(|r| {
                    let b = r["time_bucket"].as_str()?.to_string();
                    Some((b, r[alias.as_str()].as_f64().unwrap_or(0.0)))
                })
                .collect();
            pairs.sort_by(|a, b| a.0.cmp(&b.0));
            let vals: Vec<f64> = pairs.into_iter().map(|(_, v)| v).collect();
            let mid = vals.len() / 2;
            let previous: f64 = vals[..mid].iter().sum();
            let current: f64 = vals[mid..].iter().sum();
            let recent = vals[mid..].to_vec();
            (Some(current), Some(previous), recent)
        } else {
            // Aggregate: single row, first numeric value, no delta/sparkline.
            let v = resp.rows.first().and_then(first_number);
            (v, None, vec![])
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Min(0)])
            .split(inner);

        let value_span = match value {
            Some(v) => Span::styled(
                human(v),
                Style::default().fg(theme.palette.text).add_modifier(Modifier::BOLD),
            ),
            None => Span::styled("—".to_string(), Style::default().fg(theme.palette.muted)),
        };
        f.render_widget(Paragraph::new(Line::from(value_span)), chunks[0]);

        if let (Some(c), Some(p)) = (value, prev) {
            let (sym, color) = if c >= p { ("▲", theme.palette.up) } else { ("▼", theme.palette.down) };
            let pct = if p.abs() > f64::EPSILON { (c - p) / p * 100.0 } else { 0.0 };
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    format!("{sym} {:.0}%", pct.abs()),
                    Style::default().fg(color),
                ))),
                chunks[1],
            );
        }

        if !sparks.is_empty() {
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    braille_sparkline(&sparks),
                    Style::default().fg(accent(theme, 0)),
                ))),
                chunks[2],
            );
        }
    }
}

fn first_number(row: &serde_json::Value) -> Option<f64> {
    row.as_object()?.values().find_map(serde_json::Value::as_f64)
}

fn human(v: f64) -> String {
    let v = v.max(0.0);
    if v >= 1_000_000.0 {
        format!("{:.1}M", v / 1_000_000.0)
    } else if v >= 1_000.0 {
        format!("{:.1}k", v / 1_000.0)
    } else {
        format!("{v:.0}")
    }
}
```

- [ ] **Step 2: Write the failing test**

Append to `crates/gauge/src/tui/panels/stat.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::config::DashboardConfig;
    use crate::tui::data::TimeWindow;

    fn stat_spec(metric: &str) -> PanelSpec {
        let mut s = DashboardConfig::default_builtin().presets.remove(0).panels.remove(1);
        s.metric = Some(metric.into());
        s.attr = None;
        s
    }

    #[test]
    fn count_stat_emits_one_doubled_granular_request() {
        let p = Stat::from_spec(&stat_spec("events")).unwrap();
        let ctx = PanelCtx { window: TimeWindow::D7, filters: &[], meta: &[] };
        let reqs = p.data_requests(&ctx);
        assert_eq!(reqs.len(), 1);
        let req = &reqs[0].request;
        assert!(req.granularity.is_some());
        match &req.time_range {
            gauge_query::TimeRange::Last { last } => assert_eq!(last, "14d"),
            _ => panic!("expected doubled relative range"),
        }
        gauge_query::validate(req).unwrap();
    }

    #[test]
    fn aggregate_stat_without_numeric_attr_emits_nothing() {
        let p = Stat::from_spec(&stat_spec("p95")).unwrap();
        let ctx = PanelCtx { window: TimeWindow::H24, filters: &[], meta: &[] };
        assert!(p.data_requests(&ctx).is_empty());
    }

    #[test]
    fn aggregate_stat_with_attr_emits_single_window_request() {
        let mut s = stat_spec("p95");
        s.attr = Some("latency_ms".into());
        let p = Stat::from_spec(&s).unwrap();
        let ctx = PanelCtx { window: TimeWindow::H24, filters: &[], meta: &[] };
        let reqs = p.data_requests(&ctx);
        assert_eq!(reqs.len(), 1);
        assert!(reqs[0].request.granularity.is_none(), "aggregate stat is not granular");
        gauge_query::validate(&reqs[0].request).unwrap();
    }
}
```

- [ ] **Step 3: Run** — `cargo test -p gauge-client tui::panels::stat -- --nocapture` → PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/gauge/src/tui/panels/stat.rs
git commit -m "feat(tui): stat panel (deterministic doubled-window delta + sparkline)"
```

---

## Task 6: `top_n` panel

**Files:**
- Modify: `crates/gauge/src/tui/panels/top_n.rs`

- [ ] **Step 1: Replace the stub**

Replace `crates/gauge/src/tui/panels/top_n.rs` with:

```rust
//! Ranked horizontal bars for a dimension, by a count-style measure.

use ratatui::Frame;
use ratatui::layout::{Direction, Rect};
use ratatui::style::Style;
use ratatui::widgets::{Bar, BarChart, BarGroup, Paragraph};

use gauge_query::{Dimension, Field, Measure, QueryRequest};

use crate::tui::config::PanelSpec;
use crate::tui::panels::{
    LabeledRequest, Panel, PanelCtx, ResultMap, accent, base_request, count_measure, desc,
    nth_response, panel_block,
};
use crate::tui::theme::Theme;

pub struct TopN {
    title: String,
    field: Field,
    measure: Measure,
    measure_alias: String,
    limit: u32,
    pins: Vec<gauge_query::Filter>,
}

impl TopN {
    pub fn from_spec(spec: &PanelSpec) -> Result<Self, String> {
        let field = Field::parse(
            spec.field.as_deref().ok_or_else(|| "top_n: `field` is required".to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let measure_name = spec.measure.clone().unwrap_or_else(|| "count".into());
        let measure = count_measure(&measure_name)
            .ok_or_else(|| format!("top_n: bad measure `{measure_name}`"))?;
        Ok(Self {
            title: spec.title.clone().unwrap_or_else(|| field.to_string()),
            field,
            measure_alias: measure.alias(),
            measure,
            limit: spec.limit.unwrap_or(5),
            pins: spec.filters.clone(),
        })
    }

    fn request(&self, ctx: &PanelCtx) -> QueryRequest {
        let mut r = base_request(ctx, &self.pins);
        r.measures = vec![self.measure.clone()];
        r.dimensions = vec![Dimension::Field(self.field.clone())];
        r.order = vec![desc(&self.measure_alias)];
        r.limit = Some(self.limit);
        r
    }
}

impl Panel for TopN {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn data_requests(&self, ctx: &PanelCtx) -> Vec<LabeledRequest> {
        vec![LabeledRequest::new(self.request(ctx))]
    }

    fn render(&self, f: &mut Frame, area: Rect, ctx: &PanelCtx, results: &ResultMap, theme: &Theme) {
        let block = panel_block(&self.title, theme);
        let reqs = self.data_requests(ctx);
        let Some(resp) = nth_response(&reqs, 0, results) else {
            f.render_widget(Paragraph::new("loading…").block(block), area);
            return;
        };
        let dim_alias = self.field.to_string();
        let bars: Vec<Bar> = resp
            .rows
            .iter()
            .enumerate()
            .map(|(i, row)| {
                Bar::default()
                    .label(row[dim_alias.as_str()].as_str().unwrap_or("?").to_string().into())
                    .value(row[self.measure_alias.as_str()].as_i64().unwrap_or(0) as u64)
                    .style(Style::default().fg(accent(theme, i)))
            })
            .collect();
        let chart = BarChart::default()
            .block(block)
            .direction(Direction::Horizontal)
            .bar_width(1)
            .bar_gap(0)
            .data(BarGroup::default().bars(&bars));
        f.render_widget(chart, area);
    }
}
```

- [ ] **Step 2: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::config::DashboardConfig;
    use crate::tui::data::TimeWindow;

    fn spec() -> PanelSpec {
        DashboardConfig::default_builtin().presets.remove(0).panels.remove(5)
    }

    #[test]
    fn builds_ordered_limited_request() {
        let p = TopN::from_spec(&spec()).unwrap();
        let ctx = PanelCtx { window: TimeWindow::D30, filters: &[], meta: &[] };
        let req = &p.data_requests(&ctx)[0].request;
        assert_eq!(req.limit, Some(5));
        assert_eq!(req.order.len(), 1);
        assert_eq!(req.dimensions.len(), 1);
        gauge_query::validate(req).unwrap();
    }
}
```

- [ ] **Step 3: Run** — `cargo test -p gauge-client tui::panels::top_n -- --nocapture` → PASS.

- [ ] **Step 4: Commit** — `git commit -am "feat(tui): top_n ranked bars panel"` (after `git add` of the file).

---

## Task 7: `breakdown` panel

**Files:**
- Modify: `crates/gauge/src/tui/panels/breakdown.rs`

- [ ] **Step 1: Replace the stub**

```rust
//! Share-of-total percentage bars for a dimension (os / arch / version).

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use gauge_query::{Dimension, Field, QueryRequest};

use crate::tui::config::PanelSpec;
use crate::tui::panels::{
    LabeledRequest, Panel, PanelCtx, ResultMap, accent, base_request, desc, nth_response, panel_block,
};
use crate::tui::theme::Theme;

pub struct Breakdown {
    title: String,
    field: Field,
    pins: Vec<gauge_query::Filter>,
}

impl Breakdown {
    pub fn from_spec(spec: &PanelSpec) -> Result<Self, String> {
        let field = Field::parse(
            spec.field.as_deref().ok_or_else(|| "breakdown: `field` is required".to_string())?,
        )
        .map_err(|e| e.to_string())?;
        Ok(Self {
            title: spec.title.clone().unwrap_or_else(|| field.to_string()),
            field,
            pins: spec.filters.clone(),
        })
    }

    fn request(&self, ctx: &PanelCtx) -> QueryRequest {
        let mut r = base_request(ctx, &self.pins);
        r.dimensions = vec![Dimension::Field(self.field.clone())];
        r.order = vec![desc("count")];
        r.limit = Some(8);
        r
    }
}

impl Panel for Breakdown {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn data_requests(&self, ctx: &PanelCtx) -> Vec<LabeledRequest> {
        vec![LabeledRequest::new(self.request(ctx))]
    }

    fn render(&self, f: &mut Frame, area: Rect, ctx: &PanelCtx, results: &ResultMap, theme: &Theme) {
        let block = panel_block(&self.title, theme);
        let reqs = self.data_requests(ctx);
        let Some(resp) = nth_response(&reqs, 0, results) else {
            f.render_widget(Paragraph::new("loading…").block(block), area);
            return;
        };
        let dim = self.field.to_string();
        let total: f64 = resp.rows.iter().map(|r| r["count"].as_f64().unwrap_or(0.0)).sum::<f64>().max(1.0);
        let lines: Vec<Line> = resp
            .rows
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let label = r[dim.as_str()].as_str().unwrap_or("?");
                let pct = r["count"].as_f64().unwrap_or(0.0) / total * 100.0;
                Line::from(vec![
                    Span::styled(format!("{label:<10} "), Style::default().fg(theme.palette.text)),
                    Span::styled(format!("{pct:>4.0}%"), Style::default().fg(accent(theme, i))),
                ])
            })
            .collect();
        f.render_widget(Paragraph::new(lines).block(block), area);
    }
}
```

- [ ] **Step 2: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::config::DashboardConfig;
    use crate::tui::data::TimeWindow;

    fn spec() -> PanelSpec {
        DashboardConfig::default_builtin().presets.remove(0).panels.remove(7)
    }

    #[test]
    fn builds_a_grouped_request() {
        let p = Breakdown::from_spec(&spec()).unwrap();
        let ctx = PanelCtx { window: TimeWindow::D7, filters: &[], meta: &[] };
        let req = &p.data_requests(&ctx)[0].request;
        assert_eq!(req.dimensions.len(), 1);
        gauge_query::validate(req).unwrap();
    }
}
```

- [ ] **Step 3: Run** — `cargo test -p gauge-client tui::panels::breakdown -- --nocapture` → PASS.

- [ ] **Step 4: Commit** — `git commit -am "feat(tui): breakdown share-of-total panel"`.

---

## Task 8: `numeric_stats` panel

**Files:**
- Modify: `crates/gauge/src/tui/panels/numeric_stats.rs`

- [ ] **Step 1: Replace the stub**

```rust
//! Percentile / min / max / avg tiles for a numeric attr.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use gauge_query::{Field, Measure, QueryRequest};

use crate::tui::config::PanelSpec;
use crate::tui::panels::{
    LabeledRequest, Panel, PanelCtx, ResultMap, base_request, nth_response, panel_block,
    resolve_numeric_attr,
};
use crate::tui::theme::Theme;

pub struct NumericStats {
    title: String,
    explicit_attr: Option<String>,
    pins: Vec<gauge_query::Filter>,
}

impl NumericStats {
    pub fn from_spec(spec: &PanelSpec) -> Result<Self, String> {
        Ok(Self {
            title: spec.title.clone().unwrap_or_else(|| "Distribution".into()),
            explicit_attr: spec.attr.clone(),
            pins: spec.filters.clone(),
        })
    }

    fn request(&self, ctx: &PanelCtx) -> Option<QueryRequest> {
        let field = Field::Attr(resolve_numeric_attr(&self.explicit_attr, ctx.meta)?);
        let mut r = base_request(ctx, &self.pins);
        r.measures = vec![
            Measure::P50(field.clone()),
            Measure::P90(field.clone()),
            Measure::P95(field.clone()),
            Measure::P99(field.clone()),
            Measure::Min(field.clone()),
            Measure::Max(field.clone()),
            Measure::Avg(field),
        ];
        Some(r)
    }
}

impl Panel for NumericStats {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn data_requests(&self, ctx: &PanelCtx) -> Vec<LabeledRequest> {
        self.request(ctx).map(LabeledRequest::new).into_iter().collect()
    }

    fn render(&self, f: &mut Frame, area: Rect, ctx: &PanelCtx, results: &ResultMap, theme: &Theme) {
        let block = panel_block(&self.title, theme);
        let reqs = self.data_requests(ctx);
        let row = nth_response(&reqs, 0, results).and_then(|r| r.rows.first());
        let Some(row) = row else {
            let msg = if reqs.is_empty() { "no numeric attributes yet" } else { "loading…" };
            f.render_widget(
                Paragraph::new(msg).block(block).style(Style::default().fg(theme.palette.muted)),
                area,
            );
            return;
        };
        let get = |prefix: &str| -> Option<f64> {
            row.as_object()?.iter().find(|(k, _)| k.starts_with(prefix)).and_then(|(_, v)| v.as_f64())
        };
        let cell = |label: &str, v: Option<f64>| {
            Span::styled(
                format!("{label} {}  ", v.map(fmt).unwrap_or_else(|| "—".into())),
                Style::default().fg(theme.palette.text),
            )
        };
        let lines = vec![
            Line::from(vec![cell("p50", get("p50_")), cell("p90", get("p90_"))]),
            Line::from(vec![cell("p95", get("p95_")), cell("p99", get("p99_"))]),
            Line::from(Span::styled(
                format!(
                    "min {}  max {}  avg {}",
                    get("min_").map(fmt).unwrap_or_else(|| "—".into()),
                    get("max_").map(fmt).unwrap_or_else(|| "—".into()),
                    get("avg_").map(fmt).unwrap_or_else(|| "—".into()),
                ),
                Style::default().fg(theme.palette.muted).add_modifier(Modifier::DIM),
            )),
        ];
        f.render_widget(Paragraph::new(lines).block(block), area);
    }
}

fn fmt(v: f64) -> String {
    if v >= 1000.0 { format!("{:.1}k", v / 1000.0) } else { format!("{v:.0}") }
}
```

- [ ] **Step 2: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::config::DashboardConfig;
    use crate::tui::data::TimeWindow;

    fn spec_with_attr() -> PanelSpec {
        let mut s = DashboardConfig::default_builtin().presets.remove(0).panels.remove(6);
        s.attr = Some("latency_ms".into());
        s
    }

    #[test]
    fn builds_percentile_request_when_attr_present() {
        let p = NumericStats::from_spec(&spec_with_attr()).unwrap();
        let ctx = PanelCtx { window: TimeWindow::H1, filters: &[], meta: &[] };
        let reqs = p.data_requests(&ctx);
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].request.measures.len(), 7);
        gauge_query::validate(&reqs[0].request).unwrap();
    }

    #[test]
    fn emits_nothing_without_an_attr() {
        let mut s = spec_with_attr();
        s.attr = None;
        let p = NumericStats::from_spec(&s).unwrap();
        let ctx = PanelCtx { window: TimeWindow::H1, filters: &[], meta: &[] };
        assert!(p.data_requests(&ctx).is_empty());
    }
}
```

- [ ] **Step 3: Run** — `cargo test -p gauge-client tui::panels::numeric_stats -- --nocapture` → PASS.

- [ ] **Step 4: Commit** — `git commit -am "feat(tui): numeric_stats percentile panel"`.

---

## Task 9: `histogram` panel

**Files:**
- Modify: `crates/gauge/src/tui/panels/histogram.rs`

- [ ] **Step 1: Replace the stub**

```rust
//! Bucketed distribution of a numeric attr, using config-supplied edges.

use ratatui::Frame;
use ratatui::layout::{Direction, Rect};
use ratatui::style::Style;
use ratatui::widgets::{Bar, BarChart, BarGroup, Paragraph};

use gauge_query::{BucketSpec, Dimension, Field, Measure, QueryRequest};

use crate::tui::config::PanelSpec;
use crate::tui::panels::{
    LabeledRequest, Panel, PanelCtx, ResultMap, accent, base_request, nth_response, panel_block,
    resolve_numeric_attr,
};
use crate::tui::theme::Theme;

const DEFAULT_EDGES: &[f64] = &[50.0, 200.0, 600.0, 1000.0];

pub struct Histogram {
    title: String,
    explicit_attr: Option<String>,
    edges: Vec<f64>,
    pins: Vec<gauge_query::Filter>,
}

impl Histogram {
    pub fn from_spec(spec: &PanelSpec) -> Result<Self, String> {
        Ok(Self {
            title: spec.title.clone().unwrap_or_else(|| "Histogram".into()),
            explicit_attr: spec.attr.clone(),
            edges: if spec.edges.is_empty() { DEFAULT_EDGES.to_vec() } else { spec.edges.clone() },
            pins: spec.filters.clone(),
        })
    }

    fn request(&self, ctx: &PanelCtx) -> Option<QueryRequest> {
        let field = Field::Attr(resolve_numeric_attr(&self.explicit_attr, ctx.meta)?);
        let mut r = base_request(ctx, &self.pins);
        r.measures = vec![Measure::Count];
        r.dimensions = vec![Dimension::Bucket {
            bucket: BucketSpec { field, edges: self.edges.clone() },
        }];
        Some(r)
    }
}

impl Panel for Histogram {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn data_requests(&self, ctx: &PanelCtx) -> Vec<LabeledRequest> {
        self.request(ctx).map(LabeledRequest::new).into_iter().collect()
    }

    fn render(&self, f: &mut Frame, area: Rect, ctx: &PanelCtx, results: &ResultMap, theme: &Theme) {
        let block = panel_block(&self.title, theme);
        let reqs = self.data_requests(ctx);
        let Some(resp) = nth_response(&reqs, 0, results) else {
            let msg = if reqs.is_empty() { "no numeric attributes yet" } else { "loading…" };
            f.render_widget(Paragraph::new(msg).block(block), area);
            return;
        };
        let bars: Vec<Bar> = resp
            .rows
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let label = row
                    .as_object()
                    .and_then(|o| o.values().find_map(serde_json::Value::as_str))
                    .unwrap_or("?")
                    .to_string();
                Bar::default()
                    .label(label.into())
                    .value(row["count"].as_i64().unwrap_or(0) as u64)
                    .style(Style::default().fg(accent(theme, i)))
            })
            .collect();
        let chart = BarChart::default()
            .block(block)
            .direction(Direction::Horizontal)
            .bar_width(1)
            .bar_gap(0)
            .data(BarGroup::default().bars(&bars));
        f.render_widget(chart, area);
    }
}
```

- [ ] **Step 2: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::config::PanelSpec;
    use crate::tui::data::TimeWindow;

    fn spec(edges: Vec<f64>) -> PanelSpec {
        PanelSpec {
            kind: "histogram".into(),
            span: 6,
            height: None,
            title: Some("Latency".into()),
            metric: None,
            metrics: vec![],
            group_by: None,
            field: None,
            measure: None,
            limit: None,
            attr: Some("latency_ms".into()),
            edges,
            filters: vec![],
        }
    }

    #[test]
    fn builds_a_bucket_request() {
        let p = Histogram::from_spec(&spec(vec![50.0, 200.0])).unwrap();
        let ctx = PanelCtx { window: TimeWindow::D7, filters: &[], meta: &[] };
        let req = &p.data_requests(&ctx)[0].request;
        assert!(matches!(req.dimensions[0], Dimension::Bucket { .. }));
        gauge_query::validate(req).unwrap();
    }

    #[test]
    fn uses_default_edges_when_none_configured() {
        let p = Histogram::from_spec(&spec(vec![])).unwrap();
        assert_eq!(p.edges, DEFAULT_EDGES.to_vec());
    }
}
```

- [ ] **Step 3: Run** — `cargo test -p gauge-client tui::panels::histogram -- --nocapture` → PASS.

- [ ] **Step 4: Commit** — `git commit -am "feat(tui): histogram panel (config-supplied edges)"`.

---

## Task 10: `apps_table` panel

**Files:**
- Modify: `crates/gauge/src/tui/panels/apps_table.rs`

- [ ] **Step 1: Replace the stub**

```rust
//! Per-app totals table: events / installs / sessions per app for the window.

use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Paragraph, Row, Table};

use gauge_query::{Dimension, Dir, Field, Measure, Order, QueryRequest};

use crate::tui::config::PanelSpec;
use crate::tui::panels::{LabeledRequest, Panel, PanelCtx, ResultMap, base_request, nth_response, panel_block};
use crate::tui::theme::Theme;

pub struct AppsTable {
    title: String,
    pins: Vec<gauge_query::Filter>,
}

impl AppsTable {
    pub fn from_spec(spec: &PanelSpec) -> Result<Self, String> {
        Ok(Self {
            title: spec.title.clone().unwrap_or_else(|| "Apps".into()),
            pins: spec.filters.clone(),
        })
    }

    fn request(&self, ctx: &PanelCtx) -> QueryRequest {
        let mut r = base_request(ctx, &self.pins);
        r.measures = vec![Measure::Count, Measure::UniqueInstalls, Measure::UniqueSessions];
        r.dimensions = vec![Dimension::Field(Field::App)];
        r.order = vec![Order { field: "app".into(), dir: Dir::Asc }];
        r
    }
}

impl Panel for AppsTable {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn data_requests(&self, ctx: &PanelCtx) -> Vec<LabeledRequest> {
        vec![LabeledRequest::new(self.request(ctx))]
    }

    fn render(&self, f: &mut Frame, area: Rect, ctx: &PanelCtx, results: &ResultMap, theme: &Theme) {
        let block = panel_block(&self.title, theme);
        let reqs = self.data_requests(ctx);
        let Some(resp) = nth_response(&reqs, 0, results) else {
            f.render_widget(Paragraph::new("loading…").block(block), area);
            return;
        };
        let rows: Vec<Row> = resp
            .rows
            .iter()
            .map(|r| {
                Row::new(vec![
                    r["app"].as_str().unwrap_or("?").to_string(),
                    r["count"].as_i64().unwrap_or(0).to_string(),
                    r["unique_installs"].as_i64().unwrap_or(0).to_string(),
                    r["unique_sessions"].as_i64().unwrap_or(0).to_string(),
                ])
            })
            .collect();
        let table = Table::new(
            rows,
            [Constraint::Min(12), Constraint::Length(8), Constraint::Length(9), Constraint::Length(9)],
        )
        .header(
            Row::new(vec!["app", "events", "installs", "sessions"])
                .style(Style::default().fg(theme.palette.muted).add_modifier(Modifier::BOLD)),
        )
        .style(Style::default().fg(theme.palette.text))
        .block(block);
        f.render_widget(table, area);
    }
}
```

- [ ] **Step 2: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::config::PanelSpec;
    use crate::tui::data::TimeWindow;

    fn spec() -> PanelSpec {
        PanelSpec {
            kind: "apps_table".into(),
            span: 12,
            height: None,
            title: None,
            metric: None,
            metrics: vec![],
            group_by: None,
            field: None,
            measure: None,
            limit: None,
            attr: None,
            edges: vec![],
            filters: vec![],
        }
    }

    #[test]
    fn builds_per_app_totals_request() {
        let p = AppsTable::from_spec(&spec()).unwrap();
        let ctx = PanelCtx { window: TimeWindow::D7, filters: &[], meta: &[] };
        let req = &p.data_requests(&ctx)[0].request;
        assert_eq!(req.measures.len(), 3);
        assert_eq!(req.dimensions.len(), 1);
        gauge_query::validate(req).unwrap();
    }
}
```

- [ ] **Step 3: Run** — `cargo test -p gauge-client tui::panels::apps_table -- --nocapture` → PASS.

- [ ] **Step 4: Commit** — `git commit -am "feat(tui): apps_table panel"`.

---

## Task 11: Verify the `build` factory builds every default panel

**Files:** none (the real `build` body was written in Task 3 Step 1; the stubs returned `Err`, but now every panel's real `from_spec` exists).

- [ ] **Step 1: Write the test**

Add to `mod tests` in `panels/mod.rs`:

```rust
    #[test]
    fn build_constructs_every_default_panel() {
        let cfg = crate::tui::config::DashboardConfig::default_builtin();
        for panel in &cfg.active_preset().unwrap().panels {
            assert!(build(panel).is_ok(), "kind `{}` should build", panel.kind);
        }
    }

    #[test]
    fn build_rejects_unknown_kind() {
        let mut spec = crate::tui::config::DashboardConfig::default_builtin()
            .presets.remove(0).panels.remove(0);
        spec.kind = "does-not-exist".into();
        assert!(build(&spec).unwrap_err().contains("does-not-exist"));
    }
```

- [ ] **Step 2: Run** — `cargo test -p gauge-client tui::panels::tests -- --nocapture`
Expected: PASS. (If a panel fails to build, its `from_spec` is rejecting a default spec — fix that panel.)

- [ ] **Step 3: Commit** — `git commit -am "test(tui): build factory constructs all default panels"`.

---

## Task 12: Concurrent, deduplicated data layer

**Files:**
- Modify: `crates/gauge/src/tui/data.rs`

- [ ] **Step 1: Write the failing test**

Append to `crates/gauge/src/tui/data.rs`:

```rust
#[cfg(test)]
mod dash_tests {
    use super::*;
    use crate::tui::panels::{LabeledRequest, Panel, PanelCtx, ResultMap};
    use gauge_query::{QueryRequest, QueryResponse, TimeRange};

    fn req(last: &str) -> QueryRequest {
        QueryRequest {
            measures: vec![gauge_query::Measure::Count],
            dimensions: vec![],
            filters: vec![],
            time_range: TimeRange::Last { last: last.into() },
            granularity: None,
            order: vec![],
            limit: None,
        }
    }

    struct FakePanel(Vec<QueryRequest>);
    impl Panel for FakePanel {
        fn title(&self) -> String {
            "fake".into()
        }
        fn data_requests(&self, _c: &PanelCtx) -> Vec<LabeledRequest> {
            self.0.iter().cloned().map(LabeledRequest::new).collect()
        }
        fn render(
            &self,
            _f: &mut ratatui::Frame,
            _a: ratatui::layout::Rect,
            _c: &PanelCtx,
            _r: &ResultMap,
            _t: &crate::tui::theme::Theme,
        ) {
        }
    }

    struct FakeSource;
    impl QuerySource for FakeSource {
        async fn run(&self, r: &QueryRequest) -> Result<QueryResponse, String> {
            if matches!(&r.time_range, TimeRange::Last { last } if last == "boom") {
                return Err("kaboom".into());
            }
            Ok(QueryResponse { rows: vec![], truncated: false, elapsed_ms: 0, meta: None })
        }
    }

    #[test]
    fn collect_requests_dedupes_identical_queries() {
        let panels: Vec<Box<dyn Panel>> = vec![
            Box::new(FakePanel(vec![req("1d"), req("7d")])),
            Box::new(FakePanel(vec![req("1d")])),
        ];
        let ctx = PanelCtx { window: TimeWindow::D7, filters: &[], meta: &[] };
        assert_eq!(collect_requests(&panels, &ctx).len(), 2);
    }

    #[tokio::test]
    async fn fetch_all_maps_results_and_errors_by_key() {
        let reqs = vec![LabeledRequest::new(req("1d")), LabeledRequest::new(req("boom"))];
        let map = fetch_all(&FakeSource, reqs.clone()).await;
        assert_eq!(map.len(), 2);
        assert!(map.get(&reqs[0].key).unwrap().is_ok());
        assert!(map.get(&reqs[1].key).unwrap().is_err());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p gauge-client tui::data::dash_tests -- --nocapture`
Expected: FAIL to compile — `QuerySource`, `collect_requests`, `fetch_all` not found.

- [ ] **Step 3: Implement**

Append to `crates/gauge/src/tui/data.rs`:

```rust
use crate::tui::panels::{LabeledRequest, Panel, PanelCtx, ResultMap};

/// A source of query answers. `ApiClient` implements this against the server.
pub trait QuerySource {
    fn run(
        &self,
        req: &QueryRequest,
    ) -> impl std::future::Future<Output = Result<QueryResponse, String>> + Send;
}

impl QuerySource for ApiClient {
    async fn run(&self, req: &QueryRequest) -> Result<QueryResponse, String> {
        self.query(req).await.map_err(|e| e.to_string())
    }
}

/// Gather every visible panel's requests, deduplicated by key (first wins).
pub fn collect_requests(panels: &[Box<dyn Panel>], ctx: &PanelCtx) -> Vec<LabeledRequest> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for panel in panels {
        for lr in panel.data_requests(ctx) {
            if seen.insert(lr.key.clone()) {
                out.push(lr);
            }
        }
    }
    out
}

/// Run all requests concurrently, collecting them into a key→result map.
pub async fn fetch_all<Q: QuerySource>(q: &Q, requests: Vec<LabeledRequest>) -> ResultMap {
    let futs = requests.into_iter().map(|lr| async move {
        let result = q.run(&lr.request).await;
        (lr.key, result)
    });
    futures::future::join_all(futs).await.into_iter().collect()
}
```

Ensure the top-of-file `use gauge_query::{...}` includes `QueryResponse` (add it if missing).

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p gauge-client tui::data -- --nocapture`
Expected: PASS (new `dash_tests` + existing `data` tests).

- [ ] **Step 5: Commit** — `git commit -am "feat(tui): concurrent deduplicated dashboard fetch (QuerySource)"`.

---

## Task 13: Panels & data gate

**Files:** none (verification only)

- [ ] **Step 1: Build** — `cargo build -p gauge-client` → compiles.
- [ ] **Step 2: Clippy** — `cargo clippy -p gauge-client --all-targets -- -D warnings` → clean (fix inline; expect nits like needless clones, `unwrap_or_default`).
- [ ] **Step 3: Tests** — `cargo test -p gauge-client` → all pass.
- [ ] **Step 4: Old UI intact** — `git grep -n "pub async fn fetch" crates/gauge/src/tui/data.rs` shows the original `fetch` still present; `gauge tui` unchanged.
- [ ] **Step 5: Commit any fixes** — `git commit -am "chore(tui): panels + data layer pass build + clippy -D warnings"`.

---

## Done criteria for Plan 2

- `Panel` trait (`render` takes `&PanelCtx`) + `build()` factory construct all seven kinds; unknown kinds error.
- Every default panel's `data_requests` produces deterministic queries that pass `gauge_query::validate`.
- Panels look up their own results by request key (no cross-panel bleed); `stat` is delta-capable without wall-clock via the doubled-window split.
- `collect_requests` dedupes; `fetch_all` runs concurrently and maps results/errors by key.
- `cargo clippy --all-targets -- -D warnings` clean; old `gauge tui` unchanged.

**Next:** Plan 3 wires these in — `Mode{Dashboard,Explore}`, a run loop that calls `collect_requests`/`fetch_all`, the top filter/preset/window bar, the status bar, and a grid render that solves the layout (Plan 1) and calls `panel.render(f, rect, &ctx, &results, &theme)` — replacing the Overview/Apps pages.
