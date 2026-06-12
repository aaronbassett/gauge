mod common;

use axum::http::StatusCode;
use gauge_auth::{Role, mint_token};
use gauge_events::profile::{ParsedEvent, ResourceInfo};
use gauge_server::app::build_router;
use gauge_server::db;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

async fn seed(pool: &PgPool) {
    let now = OffsetDateTime::now_utc();
    // two installs for tome, one for midnight-manual
    for (app, install, n_search) in [
        ("tome", Uuid::new_v4(), 3),
        ("tome", Uuid::new_v4(), 1),
        ("midnight-manual", Uuid::new_v4(), 2),
    ] {
        let res = ResourceInfo {
            app: app.into(),
            app_version: "0.1.0".into(),
            install_id: install,
            session_id: Uuid::new_v4(),
            os: "darwin".into(),
            arch: "arm64".into(),
        };
        let mut events = Vec::new();
        for _ in 0..n_search {
            let mut attributes = serde_json::Map::new();
            attributes.insert("surface".into(), serde_json::json!("cli"));
            events.push(ParsedEvent {
                event_name: format!("{app}.search"),
                time: now,
                attributes,
            });
        }
        db::insert_events(pool, &res, &events).await.unwrap();
    }
}

fn token(state: &gauge_server::state::AppState) -> String {
    mint_token(
        &state.secret,
        "alice",
        Role::Admin,
        OffsetDateTime::now_utc(),
    )
    .unwrap()
    .0
}

#[sqlx::test(migrations = "../../migrations")]
async fn query_requires_auth(pool: PgPool) {
    let (state, _kp) = common::test_state(pool);
    let app = build_router(state);
    let body = serde_json::json!({"measures":["count"],"time_range":{"last":"1d"}});
    let (status, _) = common::send_json(&app, "POST", "/v1/query", Some(body), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "../../migrations")]
async fn aggregates_counts_and_uniques(pool: PgPool) {
    seed(&pool).await;
    let (state, _kp) = common::test_state(pool);
    let t = token(&state);
    let app = build_router(state);
    let body = serde_json::json!({
        "measures": ["count", "unique_installs"],
        "dimensions": ["app"],
        "time_range": {"last": "1d"},
        "order": [{"field": "app", "dir": "asc"}]
    });
    let (status, resp) = common::send_json(&app, "POST", "/v1/query", Some(body), Some(&t)).await;
    assert_eq!(status, StatusCode::OK);
    let rows = resp["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["app"], "midnight-manual");
    assert_eq!(rows[0]["count"], 2);
    assert_eq!(rows[0]["unique_installs"], 1);
    assert_eq!(rows[1]["app"], "tome");
    assert_eq!(rows[1]["count"], 4);
    assert_eq!(rows[1]["unique_installs"], 2);
    assert_eq!(resp["truncated"], false);
}

#[sqlx::test(migrations = "../../migrations")]
async fn attr_filter_and_dimension_work(pool: PgPool) {
    seed(&pool).await;
    let (state, _kp) = common::test_state(pool);
    let t = token(&state);
    let app = build_router(state);
    let body = serde_json::json!({
        "measures": ["count"],
        "dimensions": ["attr.surface"],
        "filters": [{"field": "app", "op": "eq", "value": "tome"}],
        "time_range": {"last": "1d"}
    });
    let (status, resp) = common::send_json(&app, "POST", "/v1/query", Some(body), Some(&t)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(resp["rows"][0]["attr.surface"], "cli");
    assert_eq!(resp["rows"][0]["count"], 4);
}

#[sqlx::test(migrations = "../../migrations")]
async fn invalid_query_is_422_naming_the_field(pool: PgPool) {
    let (state, _kp) = common::test_state(pool);
    let t = token(&state);
    let app = build_router(state);
    let body = serde_json::json!({"measures":["count"],"dimensions":["install_id"],"time_range":{"last":"1d"}});
    let (status, resp) = common::send_json(&app, "POST", "/v1/query", Some(body), Some(&t)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(resp["code"], "invalid_query");
    assert!(resp["message"].as_str().unwrap().contains("install_id"));
}
