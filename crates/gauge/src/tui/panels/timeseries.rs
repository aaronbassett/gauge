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

    fn render(
        &self,
        f: &mut Frame,
        area: Rect,
        ctx: &PanelCtx,
        results: &ResultMap,
        theme: &Theme,
    ) {
        let block = panel_block(&self.title, theme);
        let reqs = self.data_requests(ctx);
        let Some(resp) = nth_response(&reqs, 0, results) else {
            f.render_widget(
                Paragraph::new("loading…")
                    .block(block)
                    .style(Style::default().fg(theme.palette.text)),
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
            let Some(bucket) = row["time_bucket"].as_str() else {
                continue;
            };
            let x = buckets.iter().position(|b| b == bucket).unwrap_or(0) as f64;
            match &self.group_by {
                Some(field) => {
                    let name = row[field.to_string().as_str()]
                        .as_str()
                        .unwrap_or("?")
                        .to_string();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::config::DashboardConfig;
    use crate::tui::data::TimeWindow;
    use crate::tui::panels::{ResultMap, render_panel};

    fn spec() -> PanelSpec {
        DashboardConfig::default_builtin()
            .presets
            .remove(0)
            .panels
            .remove(0)
    }

    #[test]
    fn builds_a_valid_granular_request() {
        let p = Timeseries::from_spec(&spec()).unwrap();
        let ctx = PanelCtx {
            window: TimeWindow::D7,
            filters: &[],
            meta: &[],
        };
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
        let ctx = PanelCtx {
            window: TimeWindow::D7,
            filters: &[],
            meta: &[],
        };
        assert!(render_panel(&p, &ctx, &ResultMap::new(), &theme).contains("loading"));
    }
}
