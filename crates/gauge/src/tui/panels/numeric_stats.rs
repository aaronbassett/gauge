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
        let row = nth_response(&reqs, 0, results).and_then(|r| r.rows.first());
        let Some(row) = row else {
            let msg = if reqs.is_empty() {
                "no numeric attributes yet"
            } else {
                "loading…"
            };
            f.render_widget(
                Paragraph::new(msg)
                    .block(block)
                    .style(Style::default().fg(theme.palette.muted)),
                area,
            );
            return;
        };
        let get = |prefix: &str| -> Option<f64> {
            row.as_object()?
                .iter()
                .find(|(k, _)| k.starts_with(prefix))
                .and_then(|(_, v)| v.as_f64())
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
                Style::default()
                    .fg(theme.palette.muted)
                    .add_modifier(Modifier::DIM),
            )),
        ];
        f.render_widget(Paragraph::new(lines).block(block), area);
    }
}

fn fmt(v: f64) -> String {
    if v >= 1000.0 {
        format!("{:.1}k", v / 1000.0)
    } else {
        format!("{v:.0}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::config::DashboardConfig;
    use crate::tui::data::TimeWindow;

    fn spec_with_attr() -> PanelSpec {
        let mut s = DashboardConfig::default_builtin()
            .presets
            .remove(0)
            .panels
            .remove(6);
        s.attr = Some("latency_ms".into());
        s
    }

    #[test]
    fn builds_percentile_request_when_attr_present() {
        let p = NumericStats::from_spec(&spec_with_attr()).unwrap();
        let ctx = PanelCtx {
            window: TimeWindow::H1,
            filters: &[],
            meta: &[],
        };
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
        let ctx = PanelCtx {
            window: TimeWindow::H1,
            filters: &[],
            meta: &[],
        };
        assert!(p.data_requests(&ctx).is_empty());
    }
}
