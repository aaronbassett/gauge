mod common;

use axum::http::StatusCode;
use gauge_server::app::build_router;
use gauge_server::state::AppState;
use sqlx::PgPool;

fn demo_state(pool: PgPool) -> AppState {
    let (mut state, _kp) = common::test_state(pool);
    state.demo_mode = true;
    state
}

#[sqlx::test(migrations = "./migrations")]
async fn mock_is_404_when_demo_disabled(pool: PgPool) {
    let (state, _kp) = common::test_state(pool); // demo_mode = false
    let app = build_router(state);
    let (status, _) =
        common::send_json(&app, "POST", "/v1/mock", Some(serde_json::json!({})), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "./migrations")]
async fn mock_defaults_to_50_within_30_days_no_auth(pool: PgPool) {
    let app = build_router(demo_state(pool.clone()));
    // empty `{}` body, no bearer → all defaults, no auth required.
    let (status, resp) =
        common::send_json(&app, "POST", "/v1/mock", Some(serde_json::json!({})), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(resp["generated"], 50);

    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 50);

    // default window is [now-30d, now): nothing outside it.
    let outside: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM events WHERE time < now() - interval '31 days' OR time > now()",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(outside, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn mock_respects_count_and_explicit_range_and_profile(pool: PgPool) {
    let app = build_router(demo_state(pool.clone()));
    let body = serde_json::json!({
        "count": 25,
        "start": "2026-01-01T00:00:00Z",
        "end": "2026-02-01T00:00:00Z"
    });
    let (status, resp) = common::send_json(&app, "POST", "/v1/mock", Some(body), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(resp["generated"], 25);

    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 25);

    // every generated row is inside the requested half-open window
    let outside: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM events WHERE time < '2026-01-01T00:00:00Z' OR time >= '2026-02-01T00:00:00Z'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(outside, 0);

    // realistic + queryable: apps come from the allowlist, names are app-prefixed,
    // os/arch are valid Gauge-profile values.
    let bad: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM events
         WHERE app NOT IN ('tome','midnight-manual')
            OR event_name NOT LIKE app || '.%'
            OR os NOT IN ('darwin','linux','windows')
            OR arch NOT IN ('amd64','arm64')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(bad, 0);

    // installs are pooled so there is real aggregation headroom
    let installs: i64 = sqlx::query_scalar("SELECT COUNT(DISTINCT install_id) FROM events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!((1..=25).contains(&installs));
}

#[sqlx::test(migrations = "./migrations")]
async fn mock_rejects_inverted_range(pool: PgPool) {
    let app = build_router(demo_state(pool));
    let body = serde_json::json!({"start": "2026-02-01T00:00:00Z", "end": "2026-01-01T00:00:00Z"});
    let (status, resp) = common::send_json(&app, "POST", "/v1/mock", Some(body), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(resp["code"], "invalid_request");
}

#[sqlx::test(migrations = "./migrations")]
async fn mock_rejects_count_over_cap(pool: PgPool) {
    let app = build_router(demo_state(pool));
    let body = serde_json::json!({"count": 100_001});
    let (status, resp) = common::send_json(&app, "POST", "/v1/mock", Some(body), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(resp["code"], "invalid_request");
}
