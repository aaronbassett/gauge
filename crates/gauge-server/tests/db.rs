use gauge_events::profile::{ParsedEvent, ResourceInfo};
use gauge_server::db;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

pub fn test_resource(app: &str) -> ResourceInfo {
    ResourceInfo {
        app: app.into(),
        app_version: "0.1.0".into(),
        install_id: Uuid::new_v4(),
        session_id: Uuid::new_v4(),
        os: "darwin".into(),
        arch: "arm64".into(),
    }
}

pub fn test_event(app: &str, name: &str, time: OffsetDateTime) -> ParsedEvent {
    let mut attributes = serde_json::Map::new();
    attributes.insert("surface".into(), serde_json::json!("cli"));
    ParsedEvent {
        event_name: format!("{app}.{name}"),
        time,
        attributes,
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn insert_events_persists_rows(pool: PgPool) {
    let res = test_resource("tome");
    let now = OffsetDateTime::now_utc();
    let events = vec![
        test_event("tome", "search", now),
        test_event("tome", "install", now),
    ];
    db::insert_events(&pool, &res, &events).await.unwrap();

    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 2);
    let (app, name, attrs): (String, String, serde_json::Value) = sqlx::query_as(
        "SELECT app, event_name, attributes FROM events ORDER BY event_name LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(app, "tome");
    assert_eq!(name, "tome.install");
    assert_eq!(attrs["surface"], serde_json::json!("cli"));
}

#[sqlx::test(migrations = "./migrations")]
async fn insert_empty_slice_is_noop(pool: PgPool) {
    db::insert_events(&pool, &test_resource("tome"), &[])
        .await
        .unwrap();
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}
