mod common;

use axum::http::StatusCode;
use gauge_auth::{Role, mint_token};
use gauge_events::profile::{ParsedEvent, ResourceInfo};
use gauge_server::app::build_router;
use gauge_server::db;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

#[sqlx::test(migrations = "./migrations")]
async fn meta_reports_apps_events_and_keys(pool: PgPool) {
    let res = ResourceInfo {
        app: "tome".into(),
        app_version: "0.1.0".into(),
        install_id: Uuid::new_v4(),
        session_id: Uuid::new_v4(),
        os: "linux".into(),
        arch: "amd64".into(),
    };
    let mut attributes = serde_json::Map::new();
    attributes.insert("surface".into(), serde_json::json!("cli"));
    attributes.insert("latency_bucket".into(), serde_json::json!("50-200ms"));
    attributes.insert("latency_ms".into(), serde_json::json!(42));
    let ev = ParsedEvent {
        event_name: "tome.search".into(),
        time: OffsetDateTime::now_utc(),
        attributes,
    };
    db::insert_events(&pool, &res, &[ev]).await.unwrap();

    let (state, _kp) = common::test_state(pool);
    let t = mint_token(
        &state.secret,
        "alice",
        Role::Admin,
        OffsetDateTime::now_utc(),
    )
    .unwrap()
    .0;
    let app = build_router(state);
    let (status, resp) = common::send_json(&app, "GET", "/v1/meta", None, Some(&t)).await;
    assert_eq!(status, StatusCode::OK);
    let apps = resp["apps"].as_array().unwrap();
    assert_eq!(apps.len(), 1);
    assert_eq!(apps[0]["app"], "tome");
    assert_eq!(apps[0]["event_names"], serde_json::json!(["tome.search"]));
    assert_eq!(
        apps[0]["attribute_keys"],
        serde_json::json!(["latency_bucket", "latency_ms", "surface"])
    );
    let numeric_keys = apps[0]["numeric_attribute_keys"].as_array().unwrap();
    assert!(
        numeric_keys.iter().any(|k| k == "latency_ms"),
        "numeric_attribute_keys should contain 'latency_ms'"
    );
    assert!(
        !numeric_keys.iter().any(|k| k == "surface"),
        "numeric_attribute_keys should not contain string-valued key 'surface'"
    );
    assert!(
        !numeric_keys.iter().any(|k| k == "latency_bucket"),
        "numeric_attribute_keys should not contain string-valued key 'latency_bucket'"
    );
    assert_eq!(apps[0]["total_events"], 1);
    assert!(apps[0]["first_event"].is_string());
}

#[sqlx::test(migrations = "./migrations")]
async fn meta_requires_auth(pool: PgPool) {
    let (state, _kp) = common::test_state(pool);
    let app = build_router(state);
    let (status, _) = common::send_json(&app, "GET", "/v1/meta", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
