//! Integration tests for `gauge status` (network probes via wiremock) and the
//! bare `--version` / `version` output (via the built binary).

use std::process::Command;
use std::sync::OnceLock;

use gauge::config::ClientConfig;
use gauge::status::{Overall, assemble_report};
use tokio::sync::{Mutex, MutexGuard};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn env_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().await
}

fn cfg(uri: &str) -> ClientConfig {
    ClientConfig {
        server_url: uri.trim_end_matches('/').into(),
        user_id: "alice".into(),
    }
}

async fn mock_health(server: &MockServer, healthz: u16, readyz: u16) {
    Mock::given(method("GET"))
        .and(path("/healthz"))
        .respond_with(ResponseTemplate::new(healthz).set_body_string("ok"))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/readyz"))
        .respond_with(ResponseTemplate::new(readyz).set_body_string("ok"))
        .mount(server)
        .await;
}

async fn mock_auth(server: &MockServer) {
    use base64::Engine as _;
    let nonce_b64 = base64::engine::general_purpose::STANDARD_NO_PAD.encode([9u8; 32]);
    Mock::given(method("POST"))
        .and(path("/v1/auth/challenge"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "challenge_id": "00000000-0000-4000-8000-000000000001",
            "nonce_b64": nonce_b64,
            "expires_in_s": 60
        })))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/auth/verify"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "token": "test-token",
            "user_id": "alice",
            "expires_at": time::OffsetDateTime::now_utc().unix_timestamp() + 3600
        })))
        .mount(server)
        .await;
}

async fn mock_meta(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/v1/meta"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "apps": [{
                "app": "tome",
                "event_names": ["command"],
                "attribute_keys": ["name"],
                "numeric_attribute_keys": [],
                "first_event": "2026-06-01T00:00:00Z",
                "last_event": "2026-06-22T10:25:00Z",
                "total_events": 1200000
            }]
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn healthy_when_server_up_and_authed() {
    let _g = env_lock().await;
    let tmp = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("GAUGE_CONFIG_DIR", tmp.path()) };
    gauge::keys::generate("alice").unwrap();

    let server = MockServer::start().await;
    mock_health(&server, 200, 200).await;
    mock_auth(&server).await;
    mock_meta(&server).await;

    let report = assemble_report(Ok(cfg(&server.uri()))).await;
    assert_eq!(report.overall, Overall::Healthy);
    assert!(report.server.reachable && report.server.db_ready);
    assert!(report.data.available);
    assert_eq!(report.data.apps, 1);
    assert_eq!(report.data.total_events, 1_200_000);

    unsafe { std::env::remove_var("GAUGE_CONFIG_DIR") };
}

#[tokio::test]
async fn unhealthy_when_server_down() {
    let _g = env_lock().await;
    let tmp = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("GAUGE_CONFIG_DIR", tmp.path()) };

    let server = MockServer::start().await;
    mock_health(&server, 503, 503).await;

    let report = assemble_report(Ok(cfg(&server.uri()))).await;
    assert_eq!(report.overall, Overall::Unhealthy);
    assert!(!report.server.reachable);

    unsafe { std::env::remove_var("GAUGE_CONFIG_DIR") };
}

#[tokio::test]
async fn degraded_when_server_up_but_no_key() {
    let _g = env_lock().await;
    let tmp = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("GAUGE_CONFIG_DIR", tmp.path()) };
    // No key generated → /v1/meta login fails → data unavailable.

    let server = MockServer::start().await;
    mock_health(&server, 200, 200).await;

    let report = assemble_report(Ok(cfg(&server.uri()))).await;
    assert_eq!(report.overall, Overall::Degraded);
    assert!(report.server.reachable && report.server.db_ready);
    assert!(!report.data.available);

    unsafe { std::env::remove_var("GAUGE_CONFIG_DIR") };
}

#[test]
fn version_flag_and_subcommand_print_bare_version() {
    let bin = env!("CARGO_BIN_EXE_gauge");
    for args in [vec!["--version"], vec!["-V"], vec!["version"]] {
        let out = Command::new(bin).args(&args).output().unwrap();
        assert!(out.status.success(), "args {args:?} exited non-zero");
        let stdout = String::from_utf8(out.stdout).unwrap();
        assert_eq!(
            stdout.trim_end(),
            env!("CARGO_PKG_VERSION"),
            "args {args:?}"
        );
        assert!(out.stderr.is_empty(), "args {args:?} wrote to stderr");
    }
}
