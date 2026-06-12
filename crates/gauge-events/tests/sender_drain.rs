#![cfg(feature = "sender")]

use gauge_events::sender::{drain, enqueue, SenderConfig};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn cfg(tmp: &std::path::Path, endpoint: &str) -> SenderConfig {
    SenderConfig {
        endpoint: endpoint.trim_end_matches('/').to_string(),
        app: "tome".into(),
        app_version: "0.7.0".into(),
        install_id: uuid::Uuid::new_v4(),
        session_id: uuid::Uuid::new_v4(),
        os: "linux".into(),
        arch: "amd64".into(),
        queue_path: tmp.join("queue.jsonl"),
    }
}

fn attrs() -> serde_json::Map<String, serde_json::Value> {
    let mut m = serde_json::Map::new();
    m.insert("surface".into(), serde_json::json!("cli"));
    m
}

#[tokio::test]
async fn drain_posts_and_empties_queue() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/logs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .expect(1)
        .mount(&server)
        .await;
    let tmp = tempfile::tempdir().unwrap();
    let c = cfg(tmp.path(), &server.uri());
    enqueue(&c, "tome.search", attrs()).unwrap();
    enqueue(&c, "tome.install", attrs()).unwrap();

    let report = tokio::task::spawn_blocking(move || drain(&c)).await.unwrap().unwrap();
    assert_eq!(report.sent, 2);
    assert_eq!(report.remaining, 0);

    // the body that arrived was a valid Gauge batch with 2 records
    let reqs = server.received_requests().await.unwrap();
    let body: gauge_events::otlp::ExportLogsServiceRequest =
        serde_json::from_slice(&reqs[0].body).unwrap();
    let batch =
        gauge_events::profile::validate_batch(&body, &["tome".to_string()]).unwrap();
    assert_eq!(batch.events.len(), 2);
}

#[tokio::test]
async fn server_error_keeps_queue_intact() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/logs"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    let tmp = tempfile::tempdir().unwrap();
    let c = cfg(tmp.path(), &server.uri());
    enqueue(&c, "tome.search", attrs()).unwrap();
    let queue_path = c.queue_path.clone();

    let report = tokio::task::spawn_blocking(move || drain(&c)).await.unwrap().unwrap();
    assert_eq!(report.sent, 0);
    assert_eq!(report.remaining, 1); // at-least-once: nothing lost
    assert_eq!(
        gauge_events::sender::queue::read_lines(&queue_path).unwrap().len(),
        1
    );
}

#[tokio::test]
async fn https_is_required_except_loopback() {
    let tmp = tempfile::tempdir().unwrap();
    let c = cfg(tmp.path(), "http://example.com");
    enqueue(&c, "tome.search", attrs()).unwrap();
    let err = tokio::task::spawn_blocking(move || drain(&c)).await.unwrap().unwrap_err();
    assert!(err.to_string().contains("https"));
}

#[tokio::test]
async fn concurrent_drain_is_skipped_by_lock() {
    let tmp = tempfile::tempdir().unwrap();
    let c = cfg(tmp.path(), "https://gauge-telemetry.fly.dev");
    enqueue(&c, "tome.search", attrs()).unwrap();
    std::fs::write(c.queue_path.with_extension("lock"), b"pid").unwrap(); // fresh lock held
    let report = tokio::task::spawn_blocking(move || drain(&c)).await.unwrap().unwrap();
    assert!(report.skipped_lock);
    assert_eq!(report.sent, 0);
}
