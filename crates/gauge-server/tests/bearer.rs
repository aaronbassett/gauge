mod common;

use axum::http::StatusCode;
use axum::routing::get;
use axum::{Extension, Router, middleware};
use gauge_auth::{Role, mint_token};
use gauge_server::middleware::bearer::{AuthContext, require_bearer};
use sqlx::PgPool;

async fn probe(Extension(ctx): Extension<AuthContext>) -> String {
    ctx.sub
}

fn probe_router(state: gauge_server::state::AppState) -> Router {
    Router::new()
        .route("/probe", get(probe))
        .layer(middleware::from_fn_with_state(state.clone(), require_bearer))
        .with_state(state)
}

#[sqlx::test]
async fn valid_token_passes_and_injects_context(pool: PgPool) {
    let (state, _kp) = common::test_state(pool);
    let (token, _) = mint_token(&state.secret, "alice", Role::Admin, time::OffsetDateTime::now_utc()).unwrap();
    let app = probe_router(state);
    let (status, body) = common::send_json(&app, "GET", "/probe", None, Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, serde_json::Value::String("alice".into()));
}

#[sqlx::test]
async fn missing_token_is_401(pool: PgPool) {
    let (state, _kp) = common::test_state(pool);
    let app = probe_router(state);
    let (status, body) = common::send_json(&app, "GET", "/probe", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], "missing_token");
}

#[sqlx::test]
async fn garbage_token_is_401(pool: PgPool) {
    let (state, _kp) = common::test_state(pool);
    let app = probe_router(state);
    let (status, body) = common::send_json(&app, "GET", "/probe", None, Some("garbage")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], "invalid_token");
}
