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
