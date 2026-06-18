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
            // Each refresh spawns a detached fetch; overlapping fetches apply in
            // completion order ("latest completion wins"), not request order. This is
            // benign because panels recompute their result keys at render time — a
            // stale ResultMap simply misses the current keys and panels show "loading…"
            // until the freshest fetch lands, never wrong data. A generation guard could
            // make it "latest request wins" if reorder flicker ever matters.
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

        if app.config_dirty {
            app.config_dirty = false;
            app.save_error = match app.config.save() {
                Ok(()) => None,
                Err(e) => Some(format!("could not save dashboard.toml: {e}")),
            };
        }

        if app.should_quit {
            return Ok(());
        }
    }
}
