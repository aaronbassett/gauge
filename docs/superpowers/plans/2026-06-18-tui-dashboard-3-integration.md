# TUI Dashboard Redesign — Plan 3: Dashboard Integration

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire Plans 1–2 into a running, themed, configurable dashboard. Replace `Page{Overview,Apps,Explore}` with `Mode{Dashboard,Explore}`; render the configured panel grid (layout solver + panels + theme); drive it from the existing decoupled-polling loop using `collect_requests`/`fetch_all`; keep `Explore` as a re-themed second mode.

**Architecture:** `app.rs`, `ui.rs`, and `run.rs` are rewritten. `App` holds the `DashboardConfig`, resolved `Theme`, built panels + their layout `Cell`s, the global filter set (empty until Plan 4), fetched `meta` + `ResultMap`, and the retained `ExploreState`. The run loop builds requests on the main thread (panels aren't `Send`-required because the loop runs under `block_on`, not `tokio::spawn`), then spawns a fetch task. Rendering is pure: it solves the layout and calls `panel.render(f, rect, &ctx, &results, &theme)`.

**Tech Stack:** Rust 2024, `ratatui 0.29`, `crossterm`, `tokio`, `gauge-query`.

**Plan series:** Plan 3 of 5. Depends on Plans 1–2. After this, `gauge tui` shows the new dashboard. Filtering UI is Plan 4; the live menu is Plan 5. `'/'` and `'m'` are intentionally **not** bound yet.

---

## File Structure

| File | Responsibility |
|---|---|
| `crates/gauge/src/tui/app.rs` | **Rewrite.** `Mode`, `App` (config/theme/panels/cells/meta/results/filters/explore), `new`, `ctx`, `rebuild_panels`, `cycle_preset`, `cycle_numeric_attr`, `on_key`, retained `ExploreState` + `explore_request`. |
| `crates/gauge/src/tui/ui.rs` | **Rewrite.** `render` dispatch; top bar (title/mode/preset/window/filters/banner); status bar; `render_dashboard` (solve + panels); `render_explore` (themed). |
| `crates/gauge/src/tui/run.rs` | **Rewrite.** `Msg{Data,Explore,Histogram}`, `fetch_dashboard`, event loop building requests + spawning fetch, retained Explore/Histogram spawns. |

The old `data::{fetch,Snapshot}` and `data::fetch_histogram` stay (the latter still used by Explore); `fetch`/`Snapshot` become unused but remain `pub` (no dead-code warning) and are removed in Plan 5.

---

## Task 1: Rewrite `app.rs`

**Files:**
- Rewrite: `crates/gauge/src/tui/app.rs`

- [ ] **Step 1: Replace the file**

Replace `crates/gauge/src/tui/app.rs` with:

```rust
use crossterm::event::KeyCode;
use gauge_query::{AppMeta, Filter, QueryRequest};

use crate::tui::config::{self, DashboardConfig};
use crate::tui::data::TimeWindow;
use crate::tui::layout::Cell;
use crate::tui::panels::{self, Panel, PanelCtx, ResultMap};
use crate::tui::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Dashboard,
    Explore,
}

pub const EXPLORE_MEASURES: &[&str] = &[
    "count",
    "unique_installs",
    "unique_sessions", // count-style (index 0..3)
    "avg",
    "min",
    "max",
    "p95", // numeric (need a numeric_attr)
];
/// Index in EXPLORE_MEASURES at which numeric (attr-requiring) measures begin.
pub const NUMERIC_MEASURE_BASE: usize = 3;
pub const EXPLORE_DIMENSIONS: &[&str] = &["app", "event_name", "os", "arch", "app_version"];

#[derive(Debug, Default)]
pub struct ExploreState {
    pub measure_idx: usize,
    pub dimension_idx: usize,
    pub run_requested: bool,
    pub result: Option<gauge_query::QueryResponse>,
    pub numeric_attr: Option<String>,
    pub histogram_requested: bool,
    pub histogram: Option<gauge_query::QueryResponse>,
}

pub struct App {
    pub mode: Mode,
    pub config: DashboardConfig,
    pub theme: Theme,
    pub window: TimeWindow,
    /// Global filter bar (populated in Plan 4).
    pub filters: Vec<Filter>,
    pub panels: Vec<Box<dyn Panel>>,
    pub cells: Vec<Cell>,
    pub meta: Vec<AppMeta>,
    pub results: ResultMap,
    /// Some(reason) → whole-fetch failure banner (last good data retained).
    pub stale: Option<String>,
    pub config_error: Option<String>,
    pub panel_error: Option<String>,
    pub explore: ExploreState,
    pub should_quit: bool,
    pub refresh_requested: bool,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        let (config, config_error) = config::load();
        let theme = config.resolve_theme();
        let mut app = Self {
            mode: Mode::Dashboard,
            config,
            theme,
            window: TimeWindow::D7,
            filters: vec![],
            panels: vec![],
            cells: vec![],
            meta: vec![],
            results: ResultMap::new(),
            stale: None,
            config_error,
            panel_error: None,
            explore: ExploreState::default(),
            should_quit: false,
            refresh_requested: true,
        };
        app.rebuild_panels();
        app
    }

    pub fn ctx(&self) -> PanelCtx<'_> {
        PanelCtx {
            window: self.window,
            filters: &self.filters,
            meta: &self.meta,
        }
    }

    /// Rebuild the panel set + layout cells from the active preset, recording any
    /// per-panel build errors and re-resolving the theme.
    pub fn rebuild_panels(&mut self) {
        let specs = self
            .config
            .active_preset()
            .map(|p| p.panels.clone())
            .unwrap_or_default();
        self.panels.clear();
        self.cells.clear();
        let mut errs = Vec::new();
        for spec in &specs {
            match panels::build(spec) {
                Ok(p) => {
                    self.cells.push(Cell::from_spec(spec));
                    self.panels.push(p);
                }
                Err(e) => errs.push(format!("{}: {e}", spec.kind)),
            }
        }
        self.panel_error = (!errs.is_empty()).then(|| errs.join("; "));
        self.theme = self.config.resolve_theme();
    }

    fn cycle_preset(&mut self) {
        let names: Vec<String> = self.config.presets.iter().map(|p| p.name.clone()).collect();
        if names.is_empty() {
            return;
        }
        let cur = names
            .iter()
            .position(|n| *n == self.config.active_preset)
            .unwrap_or(0);
        self.config.active_preset = names[(cur + 1) % names.len()].clone();
        self.rebuild_panels();
        self.refresh_requested = true;
    }

    fn cycle_numeric_attr(&mut self) {
        let mut keys: Vec<String> = self
            .meta
            .iter()
            .flat_map(|a| a.numeric_attribute_keys.iter().cloned())
            .collect();
        keys.sort_unstable();
        keys.dedup();
        self.explore.numeric_attr = match (&self.explore.numeric_attr, keys.first()) {
            (None, Some(first)) => Some(first.clone()),
            (Some(cur), _) => {
                let next = keys
                    .iter()
                    .position(|k| k == cur)
                    .map(|i| (i + 1) % keys.len())
                    .unwrap_or(0);
                keys.get(next).cloned()
            }
            (None, None) => None,
        };
        self.explore.histogram = None;
    }

    pub fn on_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Tab => {
                self.mode = match self.mode {
                    Mode::Dashboard => Mode::Explore,
                    Mode::Explore => Mode::Dashboard,
                }
            }
            KeyCode::Char('t') => {
                self.window = self.window.next();
                self.refresh_requested = true;
                self.explore.histogram = None;
            }
            KeyCode::Char('r') => self.refresh_requested = true,
            KeyCode::Char('p') if self.mode == Mode::Dashboard => self.cycle_preset(),
            KeyCode::Up if self.mode == Mode::Explore => {
                self.explore.measure_idx = (self.explore.measure_idx + 1) % EXPLORE_MEASURES.len()
            }
            KeyCode::Down if self.mode == Mode::Explore => {
                self.explore.dimension_idx =
                    (self.explore.dimension_idx + 1) % EXPLORE_DIMENSIONS.len()
            }
            KeyCode::Enter if self.mode == Mode::Explore => self.explore.run_requested = true,
            KeyCode::Char('h')
                if self.mode == Mode::Explore && self.explore.numeric_attr.is_some() =>
            {
                self.explore.histogram_requested = true
            }
            KeyCode::Char('n') if self.mode == Mode::Explore => self.cycle_numeric_attr(),
            _ => {}
        }
    }

    /// QueryRequest for the current Explore selection (unchanged behaviour from the
    /// pre-redesign Explore page).
    pub fn explore_request(&self) -> QueryRequest {
        let measure = EXPLORE_MEASURES[self.explore.measure_idx];
        let measures: Vec<serde_json::Value> = if self.explore.measure_idx >= NUMERIC_MEASURE_BASE {
            let field_str = self.explore.numeric_attr.as_deref().map(|k| format!("attr.{k}"));
            let field_ok = field_str
                .as_deref()
                .map(|s| gauge_query::Field::parse(s).is_ok())
                .unwrap_or(false);
            if field_ok {
                vec![serde_json::json!({ measure: field_str.unwrap() })]
            } else {
                vec![serde_json::json!("count")]
            }
        } else {
            vec![serde_json::json!(measure)]
        };
        let json = serde_json::json!({
            "measures": measures,
            "dimensions": [EXPLORE_DIMENSIONS[self.explore.dimension_idx]],
            "time_range": {"last": self.window.last()},
            "limit": 50
        });
        serde_json::from_value(json).expect("explore request is always valid")
    }
}
```

- [ ] **Step 2: Write the failing tests**

Append to `crates/gauge/src/tui/app.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn app_with_default() -> App {
        let mut app = App::new();
        app.config = DashboardConfig::default_builtin();
        app.rebuild_panels();
        app
    }

    #[test]
    fn rebuild_builds_all_default_panels() {
        let app = app_with_default();
        assert_eq!(app.panels.len(), 10);
        assert_eq!(app.cells.len(), 10);
        assert!(app.panel_error.is_none());
    }

    #[test]
    fn tab_toggles_mode() {
        let mut app = app_with_default();
        assert_eq!(app.mode, Mode::Dashboard);
        app.on_key(KeyCode::Tab);
        assert_eq!(app.mode, Mode::Explore);
        app.on_key(KeyCode::Tab);
        assert_eq!(app.mode, Mode::Dashboard);
    }

    #[test]
    fn t_cycles_window_and_requests_refresh() {
        let mut app = app_with_default();
        app.refresh_requested = false;
        let before = app.window;
        app.on_key(KeyCode::Char('t'));
        assert_ne!(app.window, before);
        assert!(app.refresh_requested);
    }

    #[test]
    fn p_cycles_presets_and_rebuilds() {
        let mut app = App::new();
        // two-preset config
        let mut cfg = DashboardConfig::default_builtin();
        let mut second = cfg.presets[0].clone();
        second.name = "second".into();
        second.panels.truncate(1); // a 1-panel preset
        cfg.presets.push(second);
        app.config = cfg;
        app.config.active_preset = "default".into();
        app.rebuild_panels();
        assert_eq!(app.panels.len(), 10);

        app.on_key(KeyCode::Char('p'));
        assert_eq!(app.config.active_preset, "second");
        assert_eq!(app.panels.len(), 1);
        assert!(app.refresh_requested);
    }

    #[test]
    fn explore_attr_cycles_from_meta() {
        let mut app = app_with_default();
        app.mode = Mode::Explore;
        app.meta = vec![AppMeta {
            app: "a".into(),
            event_names: vec![],
            attribute_keys: vec![],
            numeric_attribute_keys: vec!["a".into(), "b".into()],
            first_event: None,
            last_event: None,
            total_events: 0,
        }];
        app.on_key(KeyCode::Char('n'));
        assert_eq!(app.explore.numeric_attr.as_deref(), Some("a"));
        app.on_key(KeyCode::Char('n'));
        assert_eq!(app.explore.numeric_attr.as_deref(), Some("b"));
        app.on_key(KeyCode::Char('n'));
        assert_eq!(app.explore.numeric_attr.as_deref(), Some("a")); // wraps
    }

    #[test]
    fn explore_request_validates() {
        let mut app = app_with_default();
        app.explore.numeric_attr = Some("latency_ms".to_string());
        app.explore.measure_idx = NUMERIC_MEASURE_BASE; // avg
        let req = app.explore_request();
        gauge_query::validate(&req).unwrap();
    }
}
```

- [ ] **Step 3: Run** — `cargo test -p gauge-client tui::app -- --nocapture`
Expected: FAIL to compile first (ui.rs/run.rs still reference the old `App`). That's expected — Tasks 2 and 3 rewrite them. To check `app.rs` in isolation is not practical mid-rewrite; instead proceed to Tasks 2–3 and run the suite at Task 4. **Do not commit yet** — commit after run.rs compiles (Task 3).

> Rationale: `app.rs`, `ui.rs`, and `run.rs` form one interdependent rewrite. They're separate tasks for review granularity, but the crate only compiles once all three land. Each task still shows its complete code.

---

## Task 2: Rewrite `ui.rs`

**Files:**
- Rewrite: `crates/gauge/src/tui/ui.rs`

- [ ] **Step 1: Replace the file**

Replace `crates/gauge/src/tui/ui.rs` with:

```rust
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
        if let Some(rect) = rects.get(i) {
            if rect.width > 1 && rect.height > 1 {
                panel.render(f, *rect, &ctx, &app.results, &app.theme);
            }
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
```

- [ ] **Step 2: Write the failing test**

Append to `crates/gauge/src/tui/ui.rs`:

```rust
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
```

- [ ] **Step 3:** Proceed to Task 3 (the crate won't compile until run.rs is rewritten).

---

## Task 3: Rewrite `run.rs`

**Files:**
- Rewrite: `crates/gauge/src/tui/run.rs`

- [ ] **Step 1: Replace the file**

Replace `crates/gauge/src/tui/run.rs` with:

```rust
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{Event, EventStream, KeyEventKind};
use futures::StreamExt as _;
use gauge_query::{AppMeta, QueryResponse};

use crate::api::ApiClient;
use crate::tui::app::App;
use crate::tui::data::{self, fetch_histogram};
use crate::tui::panels::{LabeledRequest, ResultMap};
use crate::tui::ui;

enum Msg {
    Data(Result<(Vec<AppMeta>, ResultMap), String>),
    Explore(Result<QueryResponse, String>),
    Histogram(Result<QueryResponse, String>),
}

/// Fetch meta + all panel requests for one dashboard refresh.
async fn fetch_dashboard(
    api: Arc<ApiClient>,
    requests: Vec<LabeledRequest>,
) -> Result<(Vec<AppMeta>, ResultMap), String> {
    let meta = api.meta().await.map_err(|e| e.to_string())?.apps;
    let results = data::fetch_all(&*api, requests).await;
    Ok((meta, results))
}

pub async fn run(api: ApiClient) -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, api).await;
    ratatui::restore();
    result
}

async fn event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    api: ApiClient,
) -> Result<(), Box<dyn std::error::Error>> {
    let api = Arc::new(api);
    let mut app = App::new();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Msg>(8);
    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_secs(30));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        if app.refresh_requested {
            app.refresh_requested = false;
            let ctx = app.ctx();
            let requests = data::collect_requests(&app.panels, &ctx);
            let api2 = api.clone();
            let tx2 = tx.clone();
            tokio::spawn(async move {
                let r = fetch_dashboard(api2, requests).await;
                let _ = tx2.send(Msg::Data(r)).await;
            });
        }
        if app.explore.run_requested {
            app.explore.run_requested = false;
            let req = app.explore_request();
            let api2 = api.clone();
            let tx2 = tx.clone();
            tokio::spawn(async move {
                let r = api2.query(&req).await.map_err(|e| e.to_string());
                let _ = tx2.send(Msg::Explore(r)).await;
            });
        }
        if app.explore.histogram_requested {
            app.explore.histogram_requested = false;
            if let Some(key) = app.explore.numeric_attr.clone() {
                let api2 = api.clone();
                let tx2 = tx.clone();
                let w = app.window;
                tokio::spawn(async move {
                    let r = fetch_histogram(&api2, w, &key).await.map_err(|e| e.to_string());
                    let _ = tx2.send(Msg::Histogram(r)).await;
                });
            }
        }

        terminal.draw(|f| ui::render(f, &app))?;

        tokio::select! {
            maybe_ev = events.next() => {
                if let Some(Ok(Event::Key(k))) = maybe_ev
                    && k.kind == KeyEventKind::Press
                {
                    app.on_key(k.code);
                }
            }
            Some(msg) = rx.recv() => match msg {
                Msg::Data(Ok((meta, results))) => {
                    let was_empty = app.meta.is_empty();
                    app.meta = meta;
                    app.results = results;
                    app.stale = None;
                    // First time meta arrives, rebuild numeric panels' requests with it.
                    if was_empty && !app.meta.is_empty() {
                        app.refresh_requested = true;
                    }
                }
                Msg::Data(Err(e)) => app.stale = Some(e),
                Msg::Explore(Ok(r)) => app.explore.result = Some(r),
                Msg::Explore(Err(e)) => app.stale = Some(e),
                Msg::Histogram(Ok(r)) => app.explore.histogram = Some(r),
                Msg::Histogram(Err(e)) => app.stale = Some(e),
            },
            _ = tick.tick() => app.refresh_requested = true,
        }

        if app.should_quit {
            return Ok(());
        }
    }
}
```

- [ ] **Step 2: Build the crate**

Run: `cargo build -p gauge-client`
Expected: compiles. Common fixes: `fetch_histogram` is re-exported from `data` (it is `pub`); the old `spawn_fetch` helper is gone; `main.rs` still calls `gauge::tui::run::run(api)` (unchanged signature).

- [ ] **Step 3: Run the full client test suite**

Run: `cargo test -p gauge-client`
Expected: PASS — including the new `tui::app` and `tui::ui` tests, and all Plan 1–2 tests. (The deleted old `app.rs`/`ui.rs` tests are replaced by the new ones.)

- [ ] **Step 4: Commit (the three-file rewrite together)**

```bash
git add crates/gauge/src/tui/app.rs crates/gauge/src/tui/ui.rs crates/gauge/src/tui/run.rs
git commit -m "feat(tui): integrate configurable dashboard (Mode, grid render, fetch loop)"
```

---

## Task 4: Integration gate + manual smoke

**Files:** none (verification only)

- [ ] **Step 1: Clippy at CI deny level**

Run: `cargo clippy -p gauge-client --all-targets -- -D warnings`
Expected: clean. Fix inline (likely: unused imports left over from the old files, `needless_borrow`). Re-run until clean.

- [ ] **Step 2: Confirm the binary builds and the entrypoint is intact**

Run: `cargo build -p gauge-client --bin gauge`
Expected: builds. `gauge tui` now launches the dashboard.

- [ ] **Step 3: Manual smoke (requires a running server with demo data)**

This step needs a live `gauge-server`. If one is available and `~/.config/gauge/config.toml` is set up:

Run: `cargo run -p gauge-client --bin gauge -- tui`
Verify by eye:
- The dashboard renders with rounded borders and the Tokyo Night palette.
- The Activity chart, four stat tiles, Top events + Latency, and OS/Arch/Versions panels appear.
- `t` cycles the window (top-right updates, panels reload).
- `tab` switches to Explore and back; `p` is a no-op with one preset.
- `q` quits cleanly (terminal restored).

If no server is available, note that this manual step is **deferred** and rely on the unit/TestBackend coverage. Do not claim the dashboard "works end-to-end" without this step — say it's unverified against a live server.

- [ ] **Step 4: Commit any clippy fixes**

```bash
git add -A
git commit -m "chore(tui): integration passes build + clippy -D warnings"
```

---

## Done criteria for Plan 3

- `gauge tui` renders the configured panel grid with the resolved theme; `tab` toggles Dashboard/Explore; `t` cycles the window; `p` cycles presets; `r` refreshes; `q` quits.
- The run loop builds requests on the main thread and fetches concurrently; per-panel results render by key; whole-fetch failures show the stale banner with last good data retained.
- `cargo test` and `cargo clippy --all-targets -- -D warnings` are clean.
- Manual smoke either passed against a live server, or is explicitly recorded as deferred.

**Next:** Plan 4 adds the global filter bar — the `'/'` overlay to add filters (field → op → value, values discovered from `/v1/meta`), `'c'` to clear, chips already rendered in the top bar, and wiring `app.filters` through `ctx` into every request (already plumbed; Plan 4 fills `app.filters`).
