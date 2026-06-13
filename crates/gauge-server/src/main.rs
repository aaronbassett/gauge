use gauge_server::app::build_router;
use gauge_server::config::Config;
use gauge_server::state::AppState;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let cfg = Config::from_env()?;
    let addr = cfg.listen_addr;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&cfg.database_url)
        .await?;
    sqlx::migrate!("../../migrations").run(&pool).await?;
    if cfg.enable_demo_mode {
        tracing::warn!(
            "DEMO MODE ENABLED (ENABLE_DEMO_MODE=1): POST /v1/mock generates synthetic events with no auth — do not enable in production"
        );
    }
    let state = AppState::from_config(cfg, pool)?;
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "gauge-server listening");
    axum::serve(listener, app).await?;
    Ok(())
}
