mod common;

use axum::http::StatusCode;
use gauge_server::app::build_router;
use sqlx::PgPool;

#[sqlx::test]
async fn healthz_and_readyz_ok(pool: PgPool) {
    let (state, _kp) = common::test_state(pool);
    let app = build_router(state);
    let (status, _) = common::send_json(&app, "GET", "/healthz", None, None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = common::send_json(&app, "GET", "/readyz", None, None).await;
    assert_eq!(status, StatusCode::OK);
}
