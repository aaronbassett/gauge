mod common;

use axum::http::StatusCode;
use gauge_server::app::build_router;
use sqlx::PgPool;

const FIXTURE: &str = include_str!("../../gauge-events/tests/fixtures/valid_batch.json");

#[sqlx::test(migrations = "../../migrations")]
async fn valid_batch_is_stored(pool: PgPool) {
    let (state, _kp) = common::test_state(pool.clone());
    let app = build_router(state);
    let body: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
    let (status, resp) = common::send_json(&app, "POST", "/v1/logs", Some(body), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(resp.get("partialSuccess").is_none());
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events").fetch_one(&pool).await.unwrap();
    assert_eq!(n, 1);
    let name: String = sqlx::query_scalar("SELECT event_name FROM events").fetch_one(&pool).await.unwrap();
    assert_eq!(name, "tome.search");
}

#[sqlx::test(migrations = "../../migrations")]
async fn unknown_app_is_rejected_whole(pool: PgPool) {
    let (state, _kp) = common::test_state(pool.clone());
    let app = build_router(state);
    let mut body: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
    body["resourceLogs"][0]["resource"]["attributes"][0]["value"]["stringValue"] = "evil-app".into();
    let (status, resp) = common::send_json(&app, "POST", "/v1/logs", Some(body), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(resp["code"], "invalid_batch");
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events").fetch_one(&pool).await.unwrap();
    assert_eq!(n, 0);
}

#[sqlx::test(migrations = "../../migrations")]
async fn bad_record_yields_partial_success(pool: PgPool) {
    let (state, _kp) = common::test_state(pool.clone());
    let app = build_router(state);
    let mut body: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
    // append a record with a wrong-prefix event name
    let mut bad = body["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0].clone();
    bad["eventName"] = "wrong.prefix".into();
    bad["attributes"] = serde_json::json!([]);
    body["resourceLogs"][0]["scopeLogs"][0]["logRecords"].as_array_mut().unwrap().push(bad);
    let (status, resp) = common::send_json(&app, "POST", "/v1/logs", Some(body), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(resp["partialSuccess"]["rejectedLogRecords"], 1);
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events").fetch_one(&pool).await.unwrap();
    assert_eq!(n, 1);
}

#[sqlx::test(migrations = "../../migrations")]
async fn malformed_json_is_400(pool: PgPool) {
    let (state, _kp) = common::test_state(pool);
    let app = build_router(state);
    let (status, resp) =
        common::send_json(&app, "POST", "/v1/logs", Some(serde_json::json!("not otlp")), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(resp["code"], "invalid_otlp");
}
