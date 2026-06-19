//! A single scalar tile: big number + Δ vs previous period + sparkline.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use gauge_query::{Field, Measure, QueryRequest, TimeRange};

use crate::tui::config::PanelSpec;
use crate::tui::panels::{
    LabeledRequest, Panel, PanelCtx, ResultMap, accent, agg_measure, base_request,
    braille_sparkline, count_measure, nth_response, panel_block, resolve_numeric_attr,
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
        let metric = spec
            .metric
            .clone()
            .ok_or_else(|| "stat: `metric` is required".to_string())?;
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
            r.time_range = TimeRange::Last {
                last: ctx.window.doubled_last().to_string(),
            };
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

    fn render(
        &self,
        f: &mut Frame,
        area: Rect,
        ctx: &PanelCtx,
        results: &ResultMap,
        theme: &Theme,
    ) {
        let block = panel_block(&self.title, theme);
        let inner = block.inner(area);
        f.render_widget(block, area);

        let reqs = self.data_requests(ctx);
        let resp = nth_response(&reqs, 0, results);
        let Some(resp) = resp else {
            let msg = if reqs.is_empty() {
                "no numeric attributes yet"
            } else {
                "loading…"
            };
            f.render_widget(
                Paragraph::new(msg).style(Style::default().fg(theme.palette.muted)),
                inner,
            );
            return;
        };

        let (value, prev, sparks): (Option<f64>, Option<f64>, Vec<f64>) = if self.is_count() {
            // Sort buckets, read the measure column per bucket, split at the midpoint.
            let alias = self
                .measure(ctx)
                .map(|m| m.alias())
                .unwrap_or_else(|| "count".into());
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
            // Split at the midpoint: recent half = current window, older half = previous.
            // On an odd bucket count the current half gets the extra bucket — a known
            // approximation for the trend arrow, noisiest at the coarse 1h window.
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
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .split(inner);

        let value_span = match value {
            Some(v) => Span::styled(
                human(v),
                Style::default()
                    .fg(theme.palette.text)
                    .add_modifier(Modifier::BOLD),
            ),
            None => Span::styled("—".to_string(), Style::default().fg(theme.palette.muted)),
        };
        f.render_widget(Paragraph::new(Line::from(value_span)), chunks[0]);

        if let (Some(c), Some(p)) = (value, prev) {
            let (sym, color) = if c >= p {
                ("▲", theme.palette.up)
            } else {
                ("▼", theme.palette.down)
            };
            let pct = if p.abs() > f64::EPSILON {
                (c - p) / p * 100.0
            } else {
                0.0
            };
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
    row.as_object()?
        .values()
        .find_map(serde_json::Value::as_f64)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::config::DashboardConfig;
    use crate::tui::data::TimeWindow;

    fn stat_spec(metric: &str) -> PanelSpec {
        let mut s = DashboardConfig::default_builtin()
            .presets
            .remove(0)
            .panels
            .remove(1);
        s.metric = Some(metric.into());
        s.attr = None;
        s
    }

    #[test]
    fn count_stat_emits_one_doubled_granular_request() {
        let p = Stat::from_spec(&stat_spec("events")).unwrap();
        let ctx = PanelCtx {
            window: TimeWindow::D7,
            filters: &[],
            meta: &[],
        };
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
        let ctx = PanelCtx {
            window: TimeWindow::H24,
            filters: &[],
            meta: &[],
        };
        assert!(p.data_requests(&ctx).is_empty());
    }

    #[test]
    fn aggregate_stat_with_attr_emits_single_window_request() {
        let mut s = stat_spec("p95");
        s.attr = Some("latency_ms".into());
        let p = Stat::from_spec(&s).unwrap();
        let ctx = PanelCtx {
            window: TimeWindow::H24,
            filters: &[],
            meta: &[],
        };
        let reqs = p.data_requests(&ctx);
        assert_eq!(reqs.len(), 1);
        assert!(
            reqs[0].request.granularity.is_none(),
            "aggregate stat is not granular"
        );
        gauge_query::validate(&reqs[0].request).unwrap();
    }
}
