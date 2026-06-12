use std::sync::OnceLock;

use gauge::tui::data::{TimeWindow, fetch};
use tokio::sync::{Mutex, MutexGuard};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn env_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().await
}

#[tokio::test]
async fn fetch_assembles_snapshot_from_query_and_meta() {
    let _g = env_lock().await;
    let tmp = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("GAUGE_CONFIG_DIR", tmp.path()) };
    gauge::keys::generate("alice").unwrap();
    let server = MockServer::start().await;
    // auth mocks (same shape as tests/api.rs)
    use base64::Engine as _;
    let nonce_b64 = base64::engine::general_purpose::STANDARD_NO_PAD.encode([9u8; 32]);
    Mock::given(method("POST")).and(path("/v1/auth/challenge"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "challenge_id": "00000000-0000-4000-8000-000000000001",
            "nonce_b64": nonce_b64, "expires_in_s": 60
        }))).mount(&server).await;
    Mock::given(method("POST")).and(path("/v1/auth/verify"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "token": "t", "user_id": "alice",
            "expires_at": time::OffsetDateTime::now_utc().unix_timestamp() + 3600
        }))).mount(&server).await;
    Mock::given(method("POST")).and(path("/v1/query"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "rows": [{"app": "tome", "count": 7, "unique_installs": 3}],
            "truncated": false, "elapsed_ms": 1
        }))).mount(&server).await;
    Mock::given(method("GET")).and(path("/v1/meta"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "apps": [{"app": "tome", "event_names": ["tome.search"], "attribute_keys": ["surface"],
                       "first_event": null, "last_event": null, "total_events": 7}]
        }))).mount(&server).await;

    let api = gauge::api::ApiClient::from_config(&gauge::config::ClientConfig {
        server_url: server.uri(), user_id: "alice".into(),
    });
    let snap = fetch(&api, TimeWindow::D7).await.unwrap();
    assert_eq!(snap.apps.len(), 1);
    assert_eq!(snap.totals[0]["count"], 7);
    assert!(!snap.timeseries.is_empty());
    assert!(!snap.top_events.is_empty());
    unsafe { std::env::remove_var("GAUGE_CONFIG_DIR") };
}

#[test]
fn time_windows_cycle_and_map_to_dsl() {
    assert_eq!(TimeWindow::H1.last(), "1h");
    assert_eq!(TimeWindow::D30.next(), TimeWindow::H1);
    assert_eq!(TimeWindow::H24.granularity(), gauge_query::Granularity::Hour);
    assert_eq!(TimeWindow::D7.granularity(), gauge_query::Granularity::Day);
}
