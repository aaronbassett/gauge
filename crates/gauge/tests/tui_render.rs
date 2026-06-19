use crossterm::event::KeyCode;
use gauge::tui::app::{App, Mode};
use gauge::tui::config::DashboardConfig;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

/// An app pinned to the built-in default dashboard (independent of any on-disk config).
fn app() -> App {
    let mut app = App::new();
    app.config = DashboardConfig::default_builtin();
    app.rebuild_panels();
    app
}

fn draw(app: &App, w: u16, h: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| gauge::tui::ui::render(f, app)).unwrap();
    let buf = terminal.backend().buffer();
    let mut s = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            s.push_str(buf[(x, y)].symbol());
        }
        s.push('\n');
    }
    s
}

#[test]
fn dashboard_renders_chrome_and_panel_titles() {
    let text = draw(&app(), 120, 40);
    assert!(text.contains("gauge"), "title");
    assert!(text.contains("preset: default"), "preset chip");
    assert!(text.contains("filters:"), "filter bar");
    assert!(text.contains("Activity"), "timeseries panel title");
    assert!(text.contains("Top events"), "top_n panel title");
}

#[test]
fn stale_banner_renders_when_set() {
    let mut app = app();
    app.stale = Some("connection refused".into());
    assert!(draw(&app, 120, 40).contains("connection refused"));
}

#[test]
fn keys_drive_state() {
    let mut app = app();
    assert_eq!(app.mode, Mode::Dashboard);
    app.on_key(KeyCode::Tab);
    assert_eq!(app.mode, Mode::Explore);

    app.refresh_requested = false;
    app.on_key(KeyCode::Char('t'));
    assert!(app.refresh_requested, "t triggers a refresh");

    app.on_key(KeyCode::Char('q'));
    assert!(app.should_quit);
}
