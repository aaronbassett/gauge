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
            spec.field
                .as_deref()
                .ok_or_else(|| "top_n: `field` is required".to_string())?,
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
                    .label(
                        row[dim_alias.as_str()]
                            .as_str()
                            .unwrap_or("?")
                            .to_string()
                            .into(),
                    )
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
            .remove(5)
    }

    #[test]
    fn builds_ordered_limited_request() {
        let p = TopN::from_spec(&spec()).unwrap();
        let ctx = PanelCtx {
            window: TimeWindow::D30,
            filters: &[],
            meta: &[],
        };
        let req = &p.data_requests(&ctx)[0].request;
        assert_eq!(req.limit, Some(5));
        assert_eq!(req.order.len(), 1);
        assert_eq!(req.dimensions.len(), 1);
        gauge_query::validate(req).unwrap();
    }
}
