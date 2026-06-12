mod common;

use axum::http::StatusCode;
use gauge_server::app::build_router;
use gauge_server::middleware::rate_limit::Limiters;
use sqlx::PgPool;
use std::sync::Arc;

#[sqlx::test]
async fn auth_endpoint_returns_429_with_retry_after(pool: PgPool) {
    let (mut state, _kp) = common::test_state(pool);
    state.limiters = Arc::new(Limiters::new(1000, 2, 1000)); // auth: burst 2
    let app = build_router(state);
    let body = serde_json::json!({"user_id": "alice"});
    for _ in 0..2 {
        let (status, _) =
            common::send_json(&app, "POST", "/v1/auth/challenge", Some(body.clone()), None).await;
        assert_eq!(status, StatusCode::OK);
    }
    let (status, resp) =
        common::send_json(&app, "POST", "/v1/auth/challenge", Some(body), None).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(resp["code"], "rate_limited");
}
