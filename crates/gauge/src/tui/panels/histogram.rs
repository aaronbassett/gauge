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
            edges: if spec.edges.is_empty() {
                DEFAULT_EDGES.to_vec()
            } else {
                spec.edges.clone()
            },
            pins: spec.filters.clone(),
        })
    }

    fn request(&self, ctx: &PanelCtx) -> Option<QueryRequest> {
        let field = Field::Attr(resolve_numeric_attr(&self.explicit_attr, ctx.meta)?);
        let mut r = base_request(ctx, &self.pins);
        r.measures = vec![Measure::Count];
        r.dimensions = vec![Dimension::Bucket {
            bucket: BucketSpec {
                field,
                edges: self.edges.clone(),
            },
        }];
        Some(r)
    }
}

impl Panel for Histogram {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn data_requests(&self, ctx: &PanelCtx) -> Vec<LabeledRequest> {
        self.request(ctx)
            .map(LabeledRequest::new)
            .into_iter()
            .collect()
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
            let msg = if reqs.is_empty() {
                "no numeric attributes yet"
            } else {
                "loading…"
            };
            f.render_widget(Paragraph::new(msg).block(block), area);
            return;
        };
        let attr_alias =
            resolve_numeric_attr(&self.explicit_attr, ctx.meta).map(|k| format!("attr.{k}"));
        let bars: Vec<Bar> = resp
            .rows
            .iter()
            .enumerate()
            .map(|(i, row)| {
                // Read the bucket label by its known column alias (`attr.<key>`); fall
                // back to the first string value only if that column is somehow absent.
                let label = attr_alias
                    .as_deref()
                    .and_then(|a| row.get(a))
                    .and_then(serde_json::Value::as_str)
                    .or_else(|| {
                        row.as_object()
                            .and_then(|o| o.values().find_map(serde_json::Value::as_str))
                    })
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
            hidden: false,
            filters: vec![],
        }
    }

    #[test]
    fn builds_a_bucket_request() {
        let p = Histogram::from_spec(&spec(vec![50.0, 200.0])).unwrap();
        let ctx = PanelCtx {
            window: TimeWindow::D7,
            filters: &[],
            meta: &[],
        };
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
