use crossterm::event::KeyCode;
use gauge_query::{AppMeta, Filter, FilterOp, FilterValue, QueryRequest};

use crate::tui::config::{self, Borders, DashboardConfig, Meters};
use crate::tui::data::TimeWindow;
use crate::tui::layout::Cell;
use crate::tui::panels::{self, Panel, PanelCtx, ResultMap};
use crate::tui::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Dashboard,
    Explore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterStep {
    Field,
    Op,
    Value,
}

/// In-progress filter being entered through the overlay.
#[derive(Debug, Clone)]
pub struct FilterDraft {
    pub step: FilterStep,
    pub fields: Vec<String>,
    pub field_idx: usize,
    pub field: Option<String>,
    pub ops: Vec<gauge_query::FilterOp>,
    pub op_idx: usize,
    pub op: Option<gauge_query::FilterOp>,
    pub values: Vec<String>,
    pub value_idx: usize,
    pub buffer: String,
}

/// Live customization menu. `focus` indexes a flat list of rows:
/// 0=preset, 1=theme, 2=borders, 3=meters, then 4.. one row per panel.
#[derive(Debug, Clone)]
pub struct MenuState {
    pub focus: usize,
}

pub const BUILTIN_THEMES: &[&str] =
    &["tokyo-night", "catppuccin-mocha", "gruvbox-dark", "nord", "ansi"];

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
    /// Config parse/load error (sticky until a successful reload).
    pub config_error: Option<String>,
    pub panel_error: Option<String>,
    /// Last live-save failure, cleared on the next successful save.
    pub save_error: Option<String>,
    pub explore: ExploreState,
    pub filter_input: Option<FilterDraft>,
    pub menu: Option<MenuState>,
    pub config_dirty: bool,
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
            save_error: None,
            explore: ExploreState::default(),
            filter_input: None,
            menu: None,
            config_dirty: false,
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
            if spec.hidden {
                continue;
            }
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

    fn menu_panel_count(&self) -> usize {
        self.config.active_preset().map(|p| p.panels.len()).unwrap_or(0)
    }

    fn menu_rows(&self) -> usize {
        4 + self.menu_panel_count()
    }

    fn active_preset_index(&self) -> Option<usize> {
        let name = &self.config.active_preset;
        self.config
            .presets
            .iter()
            .position(|p| p.name == *name)
            .or(if self.config.presets.is_empty() { None } else { Some(0) })
    }

    /// After any config mutation: rebuild panels, mark dirty (run loop persists), refresh.
    fn after_config_change(&mut self) {
        self.rebuild_panels();
        self.config_dirty = true;
        self.refresh_requested = true;
    }

    fn cycle_theme(&mut self, forward: bool) {
        let n = BUILTIN_THEMES.len();
        let cur = BUILTIN_THEMES.iter().position(|t| *t == self.config.theme.name).unwrap_or(0);
        let next = if forward { (cur + 1) % n } else { (cur + n - 1) % n };
        self.config.theme.name = BUILTIN_THEMES[next].to_string();
        self.after_config_change();
    }

    fn cycle_preset_dir(&mut self, forward: bool) {
        let names: Vec<String> = self.config.presets.iter().map(|p| p.name.clone()).collect();
        if names.is_empty() {
            return;
        }
        let n = names.len();
        let cur = names.iter().position(|x| *x == self.config.active_preset).unwrap_or(0);
        let next = if forward { (cur + 1) % n } else { (cur + n - 1) % n };
        self.config.active_preset = names[next].clone();
        self.after_config_change();
    }

    fn menu_key(&mut self, code: KeyCode) {
        let rows = self.menu_rows();
        match code {
            KeyCode::Esc | KeyCode::Char('m') => self.menu = None,
            KeyCode::Up => {
                if let Some(m) = self.menu.as_mut()
                    && rows > 0
                {
                    m.focus = (m.focus + rows - 1) % rows;
                }
            }
            KeyCode::Down => {
                if let Some(m) = self.menu.as_mut()
                    && rows > 0
                {
                    m.focus = (m.focus + 1) % rows;
                }
            }
            KeyCode::Left => self.menu_adjust(false),
            KeyCode::Right => self.menu_adjust(true),
            KeyCode::Enter | KeyCode::Char(' ') => self.menu_toggle(),
            _ => {}
        }
    }

    fn menu_adjust(&mut self, forward: bool) {
        let focus = match self.menu.as_ref() {
            Some(m) => m.focus,
            None => return,
        };
        match focus {
            0 => self.cycle_preset_dir(forward),
            1 => self.cycle_theme(forward),
            2 => {
                self.config.theme.borders = match self.config.theme.borders {
                    Borders::Rounded => Borders::Square,
                    Borders::Square => Borders::Rounded,
                };
                self.after_config_change();
            }
            3 => {
                self.config.theme.meters = match self.config.theme.meters {
                    Meters::Gradient => Meters::Solid,
                    Meters::Solid => Meters::Gradient,
                };
                self.after_config_change();
            }
            _ => {} // panel rows toggle with Enter/Space, not Left/Right
        }
    }

    fn menu_toggle(&mut self) {
        let focus = match self.menu.as_ref() {
            Some(m) => m.focus,
            None => return,
        };
        if focus < 4 {
            return;
        }
        let pidx = focus - 4;
        let Some(ai) = self.active_preset_index() else {
            return;
        };
        let toggled = if let Some(spec) = self.config.presets[ai].panels.get_mut(pidx) {
            spec.hidden = !spec.hidden;
            true
        } else {
            false
        };
        if toggled {
            self.after_config_change();
        }
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

    /// Addressable filter fields + meta attribute keys. Never includes the
    /// non-addressable `install_id`/`session_id` (privacy).
    pub fn filter_fields(&self) -> Vec<String> {
        let mut v: Vec<String> = ["app", "event_name", "os", "arch", "app_version"]
            .into_iter()
            .map(String::from)
            .collect();
        let mut attrs: Vec<String> = self
            .meta
            .iter()
            .flat_map(|m| m.attribute_keys.iter().cloned())
            .collect();
        attrs.sort_unstable();
        attrs.dedup();
        v.extend(attrs.into_iter().map(|k| format!("attr.{k}")));
        v
    }

    fn ops_for(&self, field: &str) -> Vec<FilterOp> {
        let numeric = field
            .strip_prefix("attr.")
            .map(|k| {
                self.meta
                    .iter()
                    .any(|m| m.numeric_attribute_keys.iter().any(|n| n == k))
            })
            .unwrap_or(false);
        if numeric {
            vec![
                FilterOp::Eq,
                FilterOp::Neq,
                FilterOp::Gt,
                FilterOp::Gte,
                FilterOp::Lt,
                FilterOp::Lte,
                FilterOp::Exists,
            ]
        } else {
            vec![FilterOp::Eq, FilterOp::Neq, FilterOp::In, FilterOp::Exists]
        }
    }

    fn values_for(&self, field: &str) -> Vec<String> {
        let mut v: Vec<String> = match field {
            "app" => self.meta.iter().map(|m| m.app.clone()).collect(),
            "event_name" => self
                .meta
                .iter()
                .flat_map(|m| m.event_names.iter().cloned())
                .collect(),
            _ => vec![],
        };
        v.sort_unstable();
        v.dedup();
        v
    }

    fn open_filter(&mut self) {
        let fields = self.filter_fields();
        self.filter_input = Some(FilterDraft {
            step: FilterStep::Field,
            fields,
            field_idx: 0,
            field: None,
            ops: vec![],
            op_idx: 0,
            op: None,
            values: vec![],
            value_idx: 0,
            buffer: String::new(),
        });
    }

    fn filter_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => self.filter_input = None,
            KeyCode::Enter => self.filter_advance(),
            KeyCode::Up | KeyCode::Down => {
                let down = code == KeyCode::Down;
                if let Some(d) = self.filter_input.as_mut() {
                    let len = match d.step {
                        FilterStep::Field => d.fields.len(),
                        FilterStep::Op => d.ops.len(),
                        FilterStep::Value => d.values.len(),
                    };
                    if len > 0 {
                        let idx = match d.step {
                            FilterStep::Field => &mut d.field_idx,
                            FilterStep::Op => &mut d.op_idx,
                            FilterStep::Value => &mut d.value_idx,
                        };
                        *idx = if down {
                            (*idx + 1) % len
                        } else {
                            (*idx + len - 1) % len
                        };
                    }
                }
            }
            KeyCode::Backspace => {
                if let Some(d) = self.filter_input.as_mut()
                    && d.step == FilterStep::Value
                {
                    d.buffer.pop();
                }
            }
            KeyCode::Char(c) => {
                if let Some(d) = self.filter_input.as_mut()
                    && d.step == FilterStep::Value
                {
                    d.buffer.push(c);
                }
            }
            _ => {}
        }
    }

    fn filter_advance(&mut self) {
        let step = match self.filter_input.as_ref() {
            Some(d) => d.step,
            None => return,
        };
        match step {
            FilterStep::Field => {
                let field = self
                    .filter_input
                    .as_ref()
                    .and_then(|d| d.fields.get(d.field_idx).cloned());
                if let Some(f) = field {
                    let ops = self.ops_for(&f);
                    if let Some(d) = self.filter_input.as_mut() {
                        d.field = Some(f);
                        d.ops = ops;
                        d.op_idx = 0;
                        d.step = FilterStep::Op;
                    }
                }
            }
            FilterStep::Op => {
                let op = self
                    .filter_input
                    .as_ref()
                    .and_then(|d| d.ops.get(d.op_idx).copied());
                let field = self.filter_input.as_ref().and_then(|d| d.field.clone());
                if let Some(op) = op {
                    if op == FilterOp::Exists {
                        if let Some(d) = self.filter_input.as_mut() {
                            d.op = Some(op);
                        }
                        self.commit_filter(None);
                    } else if let Some(f) = field {
                        let values = self.values_for(&f);
                        if let Some(d) = self.filter_input.as_mut() {
                            d.op = Some(op);
                            d.values = values;
                            d.value_idx = 0;
                            d.buffer.clear();
                            d.step = FilterStep::Value;
                        }
                    }
                }
            }
            FilterStep::Value => {
                let chosen = self.filter_input.as_ref().and_then(|d| {
                    if !d.buffer.is_empty() {
                        Some(d.buffer.clone())
                    } else {
                        d.values.get(d.value_idx).cloned()
                    }
                });
                if chosen.is_some() {
                    self.commit_filter(chosen);
                }
            }
        }
    }

    /// Build and push the filter, then close the overlay and refresh. Aborts (closing
    /// the overlay) on an unparseable numeric value or unknown field.
    fn commit_filter(&mut self, value: Option<String>) {
        let (field_s, op) = match self.filter_input.as_ref() {
            Some(d) => (d.field.clone(), d.op.unwrap_or(FilterOp::Eq)),
            None => return,
        };
        self.filter_input = None;
        let Some(field_s) = field_s else { return };
        let Ok(field) = gauge_query::Field::parse(&field_s) else {
            return;
        };
        let value = match op {
            FilterOp::Exists => None,
            FilterOp::Gt | FilterOp::Gte | FilterOp::Lt | FilterOp::Lte => {
                match value.as_deref().and_then(|s| s.parse::<f64>().ok()) {
                    Some(n) => Some(FilterValue::Num(n)),
                    None => return, // invalid number → abort
                }
            }
            FilterOp::In => Some(FilterValue::Many(
                value
                    .unwrap_or_default()
                    .split(',')
                    .map(|x| x.trim().to_string())
                    .filter(|x| !x.is_empty())
                    .collect(),
            )),
            FilterOp::Eq | FilterOp::Neq => value.map(FilterValue::One),
        };
        self.filters.push(Filter { field, op, value });
        self.refresh_requested = true;
    }

    pub fn on_key(&mut self, code: KeyCode) {
        if self.filter_input.is_some() {
            self.filter_key(code);
            return;
        }
        if self.menu.is_some() {
            self.menu_key(code);
            return;
        }
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
            KeyCode::Char('m') if self.mode == Mode::Dashboard => {
                self.menu = Some(MenuState { focus: 0 })
            }
            KeyCode::Char('/') if self.mode == Mode::Dashboard => self.open_filter(),
            KeyCode::Char('c') if self.mode == Mode::Dashboard => {
                if !self.filters.is_empty() {
                    self.filters.clear();
                    self.refresh_requested = true;
                }
            }
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

    fn app_with_meta() -> App {
        let mut app = app_with_default();
        app.meta = vec![AppMeta {
            app: "tome".into(),
            event_names: vec!["build".into(), "test".into()],
            attribute_keys: vec!["latency_ms".into(), "surface".into()],
            numeric_attribute_keys: vec!["latency_ms".into()],
            first_event: None,
            last_event: None,
            total_events: 0,
        }];
        app
    }

    #[test]
    fn filter_fields_exclude_identifying_fields() {
        let app = app_with_meta();
        let fields = app.filter_fields();
        assert!(fields.contains(&"app".to_string()));
        assert!(fields.contains(&"attr.latency_ms".to_string()));
        assert!(!fields.iter().any(|f| f == "install_id" || f == "session_id"));
    }

    #[test]
    fn slash_walks_field_op_value_and_commits_a_filter() {
        let mut app = app_with_meta();
        app.on_key(KeyCode::Char('/'));
        assert!(app.filter_input.is_some());
        // field: "app" is first
        app.on_key(KeyCode::Enter); // choose app → Op step
        assert_eq!(app.filter_input.as_ref().unwrap().step, FilterStep::Op);
        // op: Eq is first
        app.on_key(KeyCode::Enter); // choose Eq → Value step
        assert_eq!(app.filter_input.as_ref().unwrap().step, FilterStep::Value);
        // value: "tome" is the only suggestion
        app.on_key(KeyCode::Enter);
        assert!(app.filter_input.is_none());
        assert_eq!(app.filters.len(), 1);
        assert_eq!(app.filters[0].field, gauge_query::Field::App);
        assert_eq!(app.filters[0].op, FilterOp::Eq);
        assert!(matches!(&app.filters[0].value, Some(FilterValue::One(s)) if s == "tome"));
        assert!(app.refresh_requested);
    }

    #[test]
    fn exists_op_commits_without_a_value() {
        let mut app = app_with_meta();
        app.open_filter();
        app.on_key(KeyCode::Enter); // app → Op
        // move op selection to Exists (last in [Eq,Neq,In,Exists] = index 3)
        for _ in 0..3 {
            app.on_key(KeyCode::Down);
        }
        app.on_key(KeyCode::Enter);
        assert!(app.filter_input.is_none());
        assert_eq!(app.filters.len(), 1);
        assert_eq!(app.filters[0].op, FilterOp::Exists);
        assert!(app.filters[0].value.is_none());
    }

    #[test]
    fn numeric_attr_gt_commits_a_number_via_typed_buffer() {
        let mut app = app_with_meta();
        app.open_filter();
        // navigate to attr.latency_ms (the numeric attr; not necessarily the last field)
        let li = app
            .filter_input
            .as_ref()
            .unwrap()
            .fields
            .iter()
            .position(|f| f == "attr.latency_ms")
            .unwrap();
        for _ in 0..li {
            app.on_key(KeyCode::Down);
        }
        app.on_key(KeyCode::Enter); // → Op (numeric ops)
        // ops = [Eq,Neq,Gt,Gte,Lt,Lte,Exists]; move to Gt (index 2)
        app.on_key(KeyCode::Down);
        app.on_key(KeyCode::Down);
        app.on_key(KeyCode::Enter); // → Value
        for c in "100".chars() {
            app.on_key(KeyCode::Char(c));
        }
        app.on_key(KeyCode::Enter);
        assert_eq!(app.filters.len(), 1);
        assert_eq!(app.filters[0].op, FilterOp::Gt);
        assert!(matches!(app.filters[0].value, Some(FilterValue::Num(n)) if (n - 100.0).abs() < 1e-9));
    }

    #[test]
    fn c_clears_all_filters() {
        let mut app = app_with_meta();
        app.filters.push(Filter {
            field: gauge_query::Field::Os,
            op: FilterOp::Exists,
            value: None,
        });
        app.refresh_requested = false;
        app.on_key(KeyCode::Char('c'));
        assert!(app.filters.is_empty());
        assert!(app.refresh_requested);
    }

    #[test]
    fn esc_cancels_the_overlay_without_adding_a_filter() {
        let mut app = app_with_meta();
        app.open_filter();
        app.on_key(KeyCode::Esc);
        assert!(app.filter_input.is_none());
        assert!(app.filters.is_empty());
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

    #[test]
    fn m_opens_menu_at_first_row() {
        let mut app = app_with_default();
        app.on_key(KeyCode::Char('m'));
        assert_eq!(app.menu.as_ref().unwrap().focus, 0);
    }

    #[test]
    fn menu_cycles_theme_and_marks_dirty() {
        let mut app = app_with_default();
        app.config.theme.name = "tokyo-night".into();
        app.on_key(KeyCode::Char('m'));
        app.on_key(KeyCode::Down); // focus → theme (row 1)
        app.config_dirty = false;
        app.on_key(KeyCode::Right);
        assert_eq!(app.config.theme.name, "catppuccin-mocha");
        assert!(app.config_dirty);
        assert_eq!(app.theme.name, "catppuccin-mocha"); // resolved theme updated
    }

    #[test]
    fn menu_toggles_border_style() {
        let mut app = app_with_default();
        app.config.theme.borders = Borders::Rounded;
        app.on_key(KeyCode::Char('m'));
        app.on_key(KeyCode::Down);
        app.on_key(KeyCode::Down); // focus → borders (row 2)
        app.on_key(KeyCode::Right);
        assert_eq!(app.config.theme.borders, Borders::Square);
    }

    #[test]
    fn menu_hides_a_panel_and_rebuild_skips_it() {
        let mut app = app_with_default();
        assert_eq!(app.panels.len(), 10);
        app.on_key(KeyCode::Char('m'));
        // focus the first panel row (index 4)
        for _ in 0..4 {
            app.on_key(KeyCode::Down);
        }
        app.on_key(KeyCode::Enter); // toggle hidden on panel 0
        assert!(app.config.presets[0].panels[0].hidden);
        assert_eq!(app.panels.len(), 9, "hidden panel is skipped on rebuild");
        assert!(app.config_dirty);
    }

    #[test]
    fn esc_closes_menu() {
        let mut app = app_with_default();
        app.on_key(KeyCode::Char('m'));
        app.on_key(KeyCode::Esc);
        assert!(app.menu.is_none());
    }
}
