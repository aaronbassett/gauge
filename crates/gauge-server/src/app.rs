use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};

use crate::routes;
use crate::state::AppState;

pub fn build_router(state: AppState) -> Router {
    let public = Router::new()
        .route("/healthz", get(routes::health::healthz))
        .route("/readyz", get(routes::health::readyz));
    let ingest = Router::new()
        .route("/v1/logs", post(routes::ingest::ingest))
        .layer(DefaultBodyLimit::max(gauge_events::profile::MAX_BODY_BYTES));
    public
        .merge(ingest)
        .layer(tower_http::request_id::PropagateRequestIdLayer::x_request_id())
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .layer(tower_http::request_id::SetRequestIdLayer::x_request_id(
            tower_http::request_id::MakeRequestUuid,
        ))
        .with_state(state)
}
