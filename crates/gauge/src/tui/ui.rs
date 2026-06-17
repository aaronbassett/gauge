use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Axis, Bar, BarChart, BarGroup, Block, Borders, Chart, Dataset, GraphType, Paragraph, Row, Table,
};

use crate::tui::app::{App, EXPLORE_DIMENSIONS, EXPLORE_MEASURES, Page};

pub fn render(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(f.area());
    render_status(f, app, chunks[0]);
    match app.page {
        Page::Overview => render_overview(f, app, chunks[1]),
        Page::Apps => render_apps(f, app, chunks[1]),
        Page::Explore => render_explore(f, app, chunks[1]),
    }
}

fn render_status(f: &mut Frame, app: &App, area: Rect) {
    let mut spans = vec![
        Span::styled(" gauge ", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(format!("[{:?}] ", app.page)),
        Span::raw(format!("({}) ", app.window.label())),
        Span::raw("tab:page  t:range  r:refresh  q:quit"),
    ];
    if let Some(reason) = &app.stale {
        spans.push(Span::styled(
            format!("  STALE: {reason}"),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_overview(f: &mut Frame, app: &App, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    let bottom = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ])
        .split(rows[1]);
    render_timeseries(f, app, rows[0]);
    render_totals(f, app, bottom[0]);
    render_top_events(f, app, bottom[1]);
    render_apps_table(f, app, bottom[2]);
}

const SERIES_COLORS: &[Color] = &[
    Color::Cyan,
    Color::Magenta,
    Color::Yellow,
    Color::Green,
    Color::Blue,
];

fn render_timeseries(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Events over time");
    let Some(snap) = &app.snapshot else {
        f.render_widget(Paragraph::new("loading…").block(block), area);
        return;
    };
    // group rows by app; x = index of sorted distinct time buckets
    let mut buckets: Vec<&str> = snap
        .timeseries
        .iter()
        .filter_map(|r| r["time_bucket"].as_str())
        .collect();
    buckets.sort_unstable();
    buckets.dedup();
    let mut series: std::collections::BTreeMap<&str, Vec<(f64, f64)>> = Default::default();
    let mut y_max: f64 = 1.0;
    for row in &snap.timeseries {
        let (Some(appn), Some(bucket)) = (row["app"].as_str(), row["time_bucket"].as_str()) else {
            continue;
        };
        let count = row["count"].as_i64().unwrap_or(0) as f64;
        y_max = y_max.max(count);
        let x = buckets.iter().position(|b| *b == bucket).unwrap_or(0) as f64;
        series.entry(appn).or_default().push((x, count));
    }
    let datasets: Vec<Dataset> = series
        .iter()
        .enumerate()
        .map(|(i, (name, points))| {
            Dataset::default()
                .name(name.to_string())
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(SERIES_COLORS[i % SERIES_COLORS.len()]))
                .data(points)
        })
        .collect();
    let x_max = (buckets.len().saturating_sub(1)).max(1) as f64;
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

fn render_totals(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!("Unique installs ({})", app.window.label()));
    let Some(snap) = &app.snapshot else {
        f.render_widget(block, area);
        return;
    };
    let lines: Vec<Line> = snap
        .totals
        .iter()
        .map(|r| {
            Line::from(format!(
                "{:<18} events {:>8}   installs {:>6}   sessions {:>6}",
                r["app"].as_str().unwrap_or("?"),
                r["count"].as_i64().unwrap_or(0),
                r["unique_installs"].as_i64().unwrap_or(0),
                r["unique_sessions"].as_i64().unwrap_or(0),
            ))
        })
        .collect();
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_top_events(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default().borders(Borders::ALL).title("Top events");
    let Some(snap) = &app.snapshot else {
        f.render_widget(block, area);
        return;
    };
    let bars: Vec<Bar> = snap
        .top_events
        .iter()
        .map(|r| {
            Bar::default()
                .label(r["event_name"].as_str().unwrap_or("?").to_string().into())
                .value(r["count"].as_i64().unwrap_or(0) as u64)
        })
        .collect();
    let chart = BarChart::default()
        .block(block)
        .direction(Direction::Horizontal)
        .bar_width(1)
        .data(BarGroup::default().bars(&bars));
    f.render_widget(chart, area);
}

fn render_apps_table(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default().borders(Borders::ALL).title("Apps");
    let Some(snap) = &app.snapshot else {
        f.render_widget(block, area);
        return;
    };
    let rows: Vec<Row> = snap
        .apps
        .iter()
        .map(|a| {
            Row::new(vec![
                a.app.clone(),
                a.total_events.to_string(),
                a.event_names.len().to_string(),
                a.last_event.clone().unwrap_or_else(|| "-".into()),
            ])
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Min(16),
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Min(20),
        ],
    )
    .header(
        Row::new(vec!["app", "events", "types", "last seen"])
            .style(Style::default().add_modifier(Modifier::BOLD)),
    )
    .block(block);
    f.render_widget(table, area);
}

fn render_apps(f: &mut Frame, app: &App, area: Rect) {
    // App detail: event-name breakdown for the selected app (←/→ to switch)
    let block = Block::default()
        .borders(Borders::ALL)
        .title("App detail (←/→ to switch app)");
    let Some(snap) = &app.snapshot else {
        f.render_widget(block, area);
        return;
    };
    let Some(meta) = snap
        .apps
        .get(app.selected_app.min(snap.apps.len().saturating_sub(1)))
    else {
        f.render_widget(Paragraph::new("no apps yet").block(block), area);
        return;
    };
    let mut lines = vec![
        Line::from(Span::styled(
            meta.app.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(format!("total events: {}", meta.total_events)),
        Line::from(format!(
            "first: {}  last: {}",
            meta.first_event.as_deref().unwrap_or("-"),
            meta.last_event.as_deref().unwrap_or("-")
        )),
        Line::from(""),
        Line::from("event types:"),
    ];
    lines.extend(
        meta.event_names
            .iter()
            .map(|n| Line::from(format!("  {n}"))),
    );
    lines.push(Line::from(""));
    lines.push(Line::from(format!(
        "attribute keys: {}",
        meta.attribute_keys.join(", ")
    )));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_explore(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);
    let picker = Paragraph::new(format!(
        "measure (↑): {}    dimension (↓): {}    attr (n): {}    enter: run",
        EXPLORE_MEASURES[app.explore.measure_idx],
        EXPLORE_DIMENSIONS[app.explore.dimension_idx],
        app.explore.numeric_attr.as_deref().unwrap_or("(none)"),
    ))
    .block(Block::default().borders(Borders::ALL).title("Explore"));
    f.render_widget(picker, chunks[0]);

    let block = Block::default().borders(Borders::ALL).title("Result");
    match &app.explore.result {
        None => f.render_widget(Paragraph::new("press enter to run").block(block), chunks[1]),
        Some(resp) => {
            let lines: Vec<Line> = resp
                .rows
                .iter()
                .map(|r| Line::from(serde_json::to_string(r).unwrap_or_default()))
                .collect();
            f.render_widget(Paragraph::new(lines).block(block), chunks[1]);
        }
    }
}
