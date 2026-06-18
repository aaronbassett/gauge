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

use gauge_query::{
    AppMeta, Dir, Field, Filter, Measure, Order, QueryRequest, QueryResponse, TimeRange,
};

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
    Order {
        field: alias.to_string(),
        dir: Dir::Desc,
    }
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
            Style::default()
                .fg(theme.palette.text)
                .add_modifier(Modifier::BOLD),
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
            let m =
                |x: u8, y: u8| (x as f64 + (y as f64 - x as f64) * t.clamp(0.0, 1.0)).round() as u8;
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
            let idx = ((v / max) * (TICKS.len() as f64 - 1.0))
                .round()
                .clamp(0.0, 7.0) as usize;
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
    term.draw(|f| panel.render(f, f.area(), ctx, results, theme))
        .unwrap();
    buffer_text(term.backend().buffer())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gauge_query::FilterOp;

    fn pin(field: &str) -> Filter {
        Filter {
            field: Field::parse(field).unwrap(),
            op: FilterOp::Exists,
            value: None,
        }
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
        assert!(matches!(
            agg_measure("p95", Field::Attr("l".into())).unwrap(),
            Measure::P95(_)
        ));
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
        assert_eq!(
            resolve_numeric_attr(&Some("x".into()), &meta),
            Some("x".into())
        );
        assert_eq!(resolve_numeric_attr(&None, &meta), Some("bytes".into())); // sorted-first
        assert_eq!(resolve_numeric_attr(&None, &[]), None);
    }

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
            .presets
            .remove(0)
            .panels
            .remove(0);
        spec.kind = "does-not-exist".into();
        // `Box<dyn Panel>` is not `Debug`, so `unwrap_err()` won't type-check here;
        // match the error out directly instead.
        let Err(msg) = build(&spec) else {
            panic!("unknown kind must error");
        };
        assert!(msg.contains("does-not-exist"));
    }
}
