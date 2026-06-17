use gauge::tui::app::{App, Page};
use gauge::tui::data::{Snapshot, TimeWindow};
use gauge::tui::ui;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn synthetic_snapshot() -> Snapshot {
    Snapshot {
        fetched_at: time::OffsetDateTime::now_utc(),
        window: TimeWindow::D7,
        timeseries: vec![
            serde_json::json!({"time_bucket": "2026-06-10T00:00:00Z", "app": "tome", "count": 5}),
            serde_json::json!({"time_bucket": "2026-06-11T00:00:00Z", "app": "tome", "count": 9}),
        ],
        totals: vec![
            serde_json::json!({"app": "tome", "count": 14, "unique_installs": 4, "unique_sessions": 6}),
        ],
        top_events: vec![serde_json::json!({"event_name": "tome.search", "count": 11})],
        apps: vec![gauge_query::AppMeta {
            app: "tome".into(),
            event_names: vec!["tome.search".into()],
            attribute_keys: vec!["surface".into()],
            numeric_attribute_keys: vec![],
            first_event: None,
            last_event: None,
            total_events: 14,
        }],
    }
}

fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
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
fn overview_renders_key_widgets() {
    let mut app = App::new();
    app.snapshot = Some(synthetic_snapshot());
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| ui::render(f, &app)).unwrap();
    let text = buffer_text(&terminal);
    assert!(text.contains("Events over time"));
    assert!(text.contains("Top events"));
    assert!(text.contains("tome.search"));
    assert!(text.contains("Unique installs"));
    assert!(text.contains("last 7d"));
}

#[test]
fn stale_banner_renders_when_set() {
    let mut app = App::new();
    app.snapshot = Some(synthetic_snapshot());
    app.stale = Some("connection refused".into());
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| ui::render(f, &app)).unwrap();
    assert!(buffer_text(&terminal).contains("STALE"));
}

#[test]
fn keys_drive_state() {
    use crossterm::event::KeyCode;
    let mut app = App::new();
    assert_eq!(app.page, Page::Overview);
    app.on_key(KeyCode::Tab);
    assert_eq!(app.page, Page::Apps);
    app.on_key(KeyCode::Char('t'));
    assert!(app.refresh_requested);
    app.on_key(KeyCode::Char('q'));
    assert!(app.should_quit);
}
