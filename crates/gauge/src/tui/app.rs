use crossterm::event::KeyCode;
use gauge_query::QueryResponse;

use crate::tui::data::{Snapshot, TimeWindow};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Overview,
    Apps,
    Explore,
}

pub const EXPLORE_MEASURES: &[&str] = &["count", "unique_installs", "unique_sessions"];
pub const EXPLORE_DIMENSIONS: &[&str] = &["app", "event_name", "os", "arch", "app_version"];

#[derive(Debug, Default)]
pub struct ExploreState {
    pub measure_idx: usize,
    pub dimension_idx: usize,
    pub run_requested: bool,
    pub result: Option<QueryResponse>,
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
                let max = self.snapshot.as_ref().map(|s| s.apps.len().saturating_sub(1)).unwrap_or(0);
                self.selected_app = (self.selected_app + 1).min(max);
            }
            KeyCode::Up if self.page == Page::Explore => {
                self.explore.measure_idx = (self.explore.measure_idx + 1) % EXPLORE_MEASURES.len()
            }
            KeyCode::Down if self.page == Page::Explore => {
                self.explore.dimension_idx = (self.explore.dimension_idx + 1) % EXPLORE_DIMENSIONS.len()
            }
            KeyCode::Enter if self.page == Page::Explore => self.explore.run_requested = true,
            _ => {}
        }
    }

    /// QueryRequest for the current Explore selection.
    pub fn explore_request(&self) -> gauge_query::QueryRequest {
        let json = serde_json::json!({
            "measures": [EXPLORE_MEASURES[self.explore.measure_idx]],
            "dimensions": [EXPLORE_DIMENSIONS[self.explore.dimension_idx]],
            "time_range": {"last": self.window.last()},
            "order": [{"field": EXPLORE_MEASURES[self.explore.measure_idx], "dir": "desc"}],
            "limit": 50
        });
        serde_json::from_value(json).expect("explore request is always valid")
    }
}
