//! Per-app totals table: events / installs / sessions per app for the window.

use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Paragraph, Row, Table};

use gauge_query::{Dimension, Dir, Field, Measure, Order, QueryRequest};

use crate::tui::config::PanelSpec;
use crate::tui::panels::{
    LabeledRequest, Panel, PanelCtx, ResultMap, base_request, nth_response, panel_block,
};
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
        r.measures = vec![
            Measure::Count,
            Measure::UniqueInstalls,
            Measure::UniqueSessions,
        ];
        r.dimensions = vec![Dimension::Field(Field::App)];
        r.order = vec![Order {
            field: "app".into(),
            dir: Dir::Asc,
        }];
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
            [
                Constraint::Min(12),
                Constraint::Length(8),
                Constraint::Length(9),
                Constraint::Length(9),
            ],
        )
        .header(
            Row::new(vec!["app", "events", "installs", "sessions"]).style(
                Style::default()
                    .fg(theme.palette.muted)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .style(Style::default().fg(theme.palette.text))
        .block(block);
        f.render_widget(table, area);
    }
}

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
            hidden: false,
            filters: vec![],
        }
    }

    #[test]
    fn builds_per_app_totals_request() {
        let p = AppsTable::from_spec(&spec()).unwrap();
        let ctx = PanelCtx {
            window: TimeWindow::D7,
            filters: &[],
            meta: &[],
        };
        let req = &p.data_requests(&ctx)[0].request;
        assert_eq!(req.measures.len(), 3);
        assert_eq!(req.dimensions.len(), 1);
        gauge_query::validate(req).unwrap();
    }
}
