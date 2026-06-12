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
    let auth = Router::new()
        .route("/v1/auth/challenge", post(routes::auth::challenge))
        .route("/v1/auth/verify", post(routes::auth::verify));
    let protected = Router::new()
        .route("/v1/query", post(routes::query::query))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::middleware::bearer::require_bearer,
        ));
    public
        .merge(ingest)
        .merge(auth)
        .merge(protected)
        .layer(tower_http::request_id::PropagateRequestIdLayer::x_request_id())
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .layer(tower_http::request_id::SetRequestIdLayer::x_request_id(
            tower_http::request_id::MakeRequestUuid,
        ))
        .with_state(state)
}
