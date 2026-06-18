use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{Event, EventStream, KeyEventKind};
use futures::StreamExt as _;

use crate::api::ApiClient;
use crate::tui::app::App;
use crate::tui::data::{Snapshot, TimeWindow, fetch};
use crate::tui::ui;

enum Msg {
    Snapshot(Result<Snapshot, String>),
    Explore(Result<gauge_query::QueryResponse, String>),
    Histogram(Result<gauge_query::QueryResponse, String>),
}

fn spawn_fetch(api: Arc<ApiClient>, w: TimeWindow, tx: tokio::sync::mpsc::Sender<Msg>) {
    tokio::spawn(async move {
        let result = fetch(&api, w).await.map_err(|e| e.to_string());
        let _ = tx.send(Msg::Snapshot(result)).await;
    });
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
            spawn_fetch(api.clone(), app.window, tx.clone());
        }
        if app.explore.run_requested {
            app.explore.run_requested = false;
            let req = app.explore_request();
            let api = api.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let result = api.query(&req).await.map_err(|e| e.to_string());
                let _ = tx.send(Msg::Explore(result)).await;
            });
        }
        if app.explore.histogram_requested {
            app.explore.histogram_requested = false;
            if let Some(key) = app.explore.numeric_attr.clone() {
                let api = api.clone();
                let tx = tx.clone();
                let w = app.window;
                tokio::spawn(async move {
                    let result = crate::tui::data::fetch_histogram(&api, w, &key)
                        .await
                        .map_err(|e| e.to_string());
                    let _ = tx.send(Msg::Histogram(result)).await;
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
                Msg::Snapshot(Ok(s)) => { app.snapshot = Some(s); app.stale = None; }
                Msg::Snapshot(Err(e)) => app.stale = Some(e),
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
