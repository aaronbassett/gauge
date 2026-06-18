use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Bar, BarChart, BarGroup, Block, Paragraph};

use gauge_query::{Filter, FilterOp, FilterValue};

use crate::tui::app::{App, EXPLORE_DIMENSIONS, EXPLORE_MEASURES, Mode, NUMERIC_MEASURE_BASE};
use crate::tui::layout::solve;
use crate::tui::panels::panel_block;

pub fn render(f: &mut Frame, app: &App) {
    let area = f.area();
    // Paint the themed background (Color::Reset for the ANSI theme leaves the terminal's own bg).
    f.render_widget(
        Block::default().style(Style::default().bg(app.theme.palette.bg)),
        area,
    );
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(0), Constraint::Length(1)])
        .split(area);
    render_top_bar(f, app, chunks[0]);
    match app.mode {
        Mode::Dashboard => render_dashboard(f, app, chunks[1]),
        Mode::Explore => render_explore(f, app, chunks[1]),
    }
    render_status_bar(f, app, chunks[2]);
}

fn render_top_bar(f: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let mode = match app.mode {
        Mode::Dashboard => "dashboard",
        Mode::Explore => "explore",
    };
    let mut line1 = vec![
        Span::styled(
            " gauge ",
            Style::default()
                .fg(t.palette.bg)
                .bg(t.palette.accents[0])
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  ▸ {mode}"),
            Style::default().fg(t.palette.text).add_modifier(Modifier::BOLD),
        ),
    ];
    if app.mode == Mode::Dashboard {
        line1.push(Span::styled(
            format!("   preset: {}", app.config.active_preset),
            Style::default().fg(t.palette.muted),
        ));
    }
    line1.push(Span::styled(
        format!("   {}", app.window.label()),
        Style::default().fg(t.palette.accents[1 % t.palette.accents.len().max(1)]),
    ));

    let mut line2: Vec<Span> = vec![Span::styled(" filters: ", Style::default().fg(t.palette.muted))];
    if app.filters.is_empty() {
        line2.push(Span::styled("(none)", Style::default().fg(t.palette.muted)));
    } else {
        for fl in &app.filters {
            line2.push(Span::styled(
                format!(" {} ", filter_chip(fl)),
                Style::default().fg(t.palette.text).bg(t.palette.surface),
            ));
            line2.push(Span::raw(" "));
        }
    }
    if let Some(banner) = app
        .stale
        .as_ref()
        .or(app.config_error.as_ref())
        .or(app.panel_error.as_ref())
    {
        line2.push(Span::styled(
            format!("   ⚠ {banner}"),
            Style::default().fg(t.palette.down).add_modifier(Modifier::BOLD),
        ));
    }

    f.render_widget(
        Paragraph::new(vec![Line::from(line1), Line::from(line2)]),
        area,
    );
}

fn filter_chip(fl: &Filter) -> String {
    let op = match fl.op {
        FilterOp::Eq => "=",
        FilterOp::Neq => "≠",
        FilterOp::In => "in",
        FilterOp::Exists => "?",
        FilterOp::Gt => ">",
        FilterOp::Gte => "≥",
        FilterOp::Lt => "<",
        FilterOp::Lte => "≤",
    };
    let val = match &fl.value {
        Some(FilterValue::One(s)) => s.clone(),
        Some(FilterValue::Many(v)) => format!("{{{}}}", v.join(",")),
        Some(FilterValue::Num(n)) => n.to_string(),
        None => String::new(),
    };
    if val.is_empty() {
        format!("{} {op}", fl.field)
    } else {
        format!("{} {op} {val}", fl.field)
    }
}

fn render_dashboard(f: &mut Frame, app: &App, area: Rect) {
    if app.panels.is_empty() {
        let msg = app
            .config_error
            .clone()
            .or_else(|| app.panel_error.clone())
            .unwrap_or_else(|| "no panels configured".into());
        f.render_widget(
            Paragraph::new(msg).block(panel_block("dashboard", &app.theme)),
            area,
        );
        return;
    }
    let rects = solve(area, &app.cells);
    let ctx = app.ctx();
    for (i, panel) in app.panels.iter().enumerate() {
        if let Some(rect) = rects.get(i)
            && rect.width > 1
            && rect.height > 1
        {
            panel.render(f, *rect, &ctx, &app.results, &app.theme);
        }
    }
}

fn render_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let hints = match app.mode {
        Mode::Dashboard => "tab:explore   p:preset   t:range   r:refresh   q:quit",
        Mode::Explore => "tab:dashboard   ↑:measure   ↓:dim   n:attr   enter:run   h:hist   t:range   q:quit",
    };
    f.render_widget(
        Paragraph::new(Span::styled(
            format!(" {hints}"),
            Style::default().fg(app.theme.palette.muted),
        ))
        .style(Style::default().bg(app.theme.palette.surface)),
        area,
    );
}

fn render_explore(f: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    let needs_attr =
        app.explore.measure_idx >= NUMERIC_MEASURE_BASE && app.explore.numeric_attr.is_none();
    let attr_display = if needs_attr {
        format!("(none — pick one to run {})", EXPLORE_MEASURES[app.explore.measure_idx])
    } else {
        app.explore.numeric_attr.clone().unwrap_or_else(|| "(none)".into())
    };
    let picker = Paragraph::new(format!(
        "measure (↑): {}    dimension (↓): {}    attr (n): {}    enter: run",
        EXPLORE_MEASURES[app.explore.measure_idx],
        EXPLORE_DIMENSIONS[app.explore.dimension_idx],
        attr_display,
    ))
    .style(Style::default().fg(t.palette.text))
    .block(panel_block("Explore", t));
    f.render_widget(picker, chunks[0]);

    if let Some(hist) = &app.explore.histogram {
        let block = panel_block("Histogram (h to refresh)", t);
        if hist.rows.is_empty() {
            f.render_widget(Paragraph::new("no data for this attribute").block(block), chunks[1]);
            return;
        }
        let attr_alias = app
            .explore
            .numeric_attr
            .as_ref()
            .map(|k| format!("attr.{k}"))
            .unwrap_or_default();
        let bars: Vec<Bar> = hist
            .rows
            .iter()
            .enumerate()
            .map(|(i, r)| {
                Bar::default()
                    .label(r[attr_alias.as_str()].as_str().unwrap_or("?").to_string().into())
                    .value(r["count"].as_i64().unwrap_or(0) as u64)
                    .style(Style::default().fg(crate::tui::panels::accent(t, i)))
            })
            .collect();
        let chart = BarChart::default()
            .block(block)
            .direction(Direction::Horizontal)
            .bar_width(1)
            .data(BarGroup::default().bars(&bars));
        f.render_widget(chart, chunks[1]);
        return;
    }

    let block = panel_block("Result", t);
    match &app.explore.result {
        None => f.render_widget(
            Paragraph::new("press enter to run · n: pick attr · h: histogram")
                .style(Style::default().fg(t.palette.muted))
                .block(block),
            chunks[1],
        ),
        Some(resp) => {
            let lines: Vec<Line> = resp
                .rows
                .iter()
                .map(|r| Line::from(serde_json::to_string(r).unwrap_or_default()))
                .collect();
            f.render_widget(
                Paragraph::new(lines).style(Style::default().fg(t.palette.text)).block(block),
                chunks[1],
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::App;
    use crate::tui::config::DashboardConfig;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn draw(app: &App, w: u16, h: u16) -> String {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| render(f, app)).unwrap();
        let buf = term.backend().buffer();
        let area = buf.area;
        let mut s = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                s.push_str(buf[(x, y)].symbol());
            }
        }
        s
    }

    fn app() -> App {
        let mut a = App::new();
        a.config = DashboardConfig::default_builtin();
        a.rebuild_panels();
        a
    }

    #[test]
    fn dashboard_shows_chrome_and_panels() {
        let out = draw(&app(), 120, 40);
        assert!(out.contains("gauge"));
        assert!(out.contains("preset: default"));
        assert!(out.contains("filters:"));
        assert!(out.contains("Activity")); // timeseries panel title
        assert!(out.contains("Top events"));
    }

    #[test]
    fn explore_mode_shows_picker() {
        let mut a = app();
        a.mode = Mode::Explore;
        let out = draw(&a, 100, 24);
        assert!(out.contains("Explore"));
        assert!(out.contains("measure"));
    }
}
