//! Share-of-total percentage bars for a dimension (os / arch / version).

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use gauge_query::{Dimension, Field, QueryRequest};

use crate::tui::config::PanelSpec;
use crate::tui::panels::{
    LabeledRequest, Panel, PanelCtx, ResultMap, accent, base_request, desc, nth_response,
    panel_block,
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
            spec.field
                .as_deref()
                .ok_or_else(|| "breakdown: `field` is required".to_string())?,
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
        let total: f64 = resp
            .rows
            .iter()
            .map(|r| r["count"].as_f64().unwrap_or(0.0))
            .sum::<f64>()
            .max(1.0);
        let lines: Vec<Line> = resp
            .rows
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let label = r[dim.as_str()].as_str().unwrap_or("?");
                let pct = r["count"].as_f64().unwrap_or(0.0) / total * 100.0;
                Line::from(vec![
                    Span::styled(
                        format!("{label:<10} "),
                        Style::default().fg(theme.palette.text),
                    ),
                    Span::styled(format!("{pct:>4.0}%"), Style::default().fg(accent(theme, i))),
                ])
            })
            .collect();
        f.render_widget(Paragraph::new(lines).block(block), area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::config::DashboardConfig;
    use crate::tui::data::TimeWindow;

    fn spec() -> PanelSpec {
        DashboardConfig::default_builtin()
            .presets
            .remove(0)
            .panels
            .remove(7)
    }

    #[test]
    fn builds_a_grouped_request() {
        let p = Breakdown::from_spec(&spec()).unwrap();
        let ctx = PanelCtx {
            window: TimeWindow::D7,
            filters: &[],
            meta: &[],
        };
        let req = &p.data_requests(&ctx)[0].request;
        assert_eq!(req.dimensions.len(), 1);
        gauge_query::validate(req).unwrap();
    }
}
