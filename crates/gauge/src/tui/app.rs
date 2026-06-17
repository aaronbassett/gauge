use crossterm::event::KeyCode;
use gauge_query::QueryResponse;

use crate::tui::data::{Snapshot, TimeWindow};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Overview,
    Apps,
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
    pub result: Option<QueryResponse>,
    /// Selected numeric attribute key (from get_meta), required by numeric measures/histogram.
    pub numeric_attr: Option<String>,
}

pub struct App {
    pub page: Page,
    pub window: TimeWindow,
    pub snapshot: Option<Snapshot>,
    /// Some(reason) → keep last snapshot, show stale banner.
    pub stale: Option<String>,
    pub selected_app: usize,
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
        Self {
            page: Page::Overview,
            window: TimeWindow::D7,
            snapshot: None,
            stale: None,
            selected_app: 0,
            explore: ExploreState::default(),
            should_quit: false,
            refresh_requested: true, // fetch immediately on start
        }
    }

    pub fn on_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Tab => {
                self.page = match self.page {
                    Page::Overview => Page::Apps,
                    Page::Apps => Page::Explore,
                    Page::Explore => Page::Overview,
                }
            }
            KeyCode::Char('t') => {
                self.window = self.window.next();
                self.refresh_requested = true;
            }
            KeyCode::Char('r') => self.refresh_requested = true,
            KeyCode::Left if self.page == Page::Apps => {
                self.selected_app = self.selected_app.saturating_sub(1)
            }
            KeyCode::Right if self.page == Page::Apps => {
                let max = self
                    .snapshot
                    .as_ref()
                    .map(|s| s.apps.len().saturating_sub(1))
                    .unwrap_or(0);
                self.selected_app = (self.selected_app + 1).min(max);
            }
            KeyCode::Up if self.page == Page::Explore => {
                self.explore.measure_idx = (self.explore.measure_idx + 1) % EXPLORE_MEASURES.len()
            }
            KeyCode::Down if self.page == Page::Explore => {
                self.explore.dimension_idx =
                    (self.explore.dimension_idx + 1) % EXPLORE_DIMENSIONS.len()
            }
            KeyCode::Enter if self.page == Page::Explore => self.explore.run_requested = true,
            KeyCode::Char('n') if self.page == Page::Explore => {
                let keys: Vec<String> = self
                    .snapshot
                    .as_ref()
                    .map(|s| {
                        let mut v: Vec<String> = s
                            .apps
                            .iter()
                            .flat_map(|a| a.numeric_attribute_keys.iter().cloned())
                            .collect();
                        v.sort_unstable();
                        v.dedup();
                        v
                    })
                    .unwrap_or_default();
                self.explore.numeric_attr = match (&self.explore.numeric_attr, keys.first()) {
                    (None, Some(first)) => Some(first.clone()),
                    (Some(cur), _) => {
                        let next = keys
                            .iter()
                            .position(|k| k == cur)
                            .map(|i| (i + 1) % keys.len().max(1));
                        next.and_then(|i| keys.get(i).cloned())
                    }
                    (None, None) => None,
                };
            }
            _ => {}
        }
    }

    /// QueryRequest for the current Explore selection.
    pub fn explore_request(&self) -> gauge_query::QueryRequest {
        let measure = EXPLORE_MEASURES[self.explore.measure_idx];
        let measures: Vec<serde_json::Value> = if self.explore.measure_idx >= NUMERIC_MEASURE_BASE {
            // numeric aggregate: {"avg":"attr.<key>"}; fall back to count if no attr chosen
            match &self.explore.numeric_attr {
                Some(key) => vec![serde_json::json!({ measure: format!("attr.{key}") })],
                None => vec![serde_json::json!("count")],
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

    #[test]
    fn explore_request_supports_numeric_aggregate() {
        let mut app = App::new();
        app.explore.numeric_attr = Some("latency_ms".to_string());
        app.explore.measure_idx = NUMERIC_MEASURE_BASE; // first numeric measure (avg)
        let req = app.explore_request();
        // an avg over attr.latency_ms is built and validates
        assert!(req.measures.iter().any(
            |m| matches!(m, gauge_query::Measure::Avg(f) if f.to_string() == "attr.latency_ms")
        ));
        gauge_query::validate(&req).unwrap();
    }
}
