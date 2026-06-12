use std::sync::{Mutex, OnceLock};

use gauge::api::ApiClient;
use gauge::config::ClientConfig;
use gauge_auth::{Keypair, verify_signature};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}

const NONCE: [u8; 32] = [9u8; 32];

async fn mock_auth(server: &MockServer) {
    use base64::Engine as _;
    let nonce_b64 = base64::engine::general_purpose::STANDARD_NO_PAD.encode(NONCE);
    Mock::given(method("POST")).and(path("/v1/auth/challenge"))
        .and(body_partial_json(serde_json::json!({"user_id": "alice"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "challenge_id": "00000000-0000-4000-8000-000000000001",
            "nonce_b64": nonce_b64,
            "expires_in_s": 60
        })))
        .mount(server).await;
    Mock::given(method("POST")).and(path("/v1/auth/verify"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "token": "test-token",
            "user_id": "alice",
            "expires_at": time::OffsetDateTime::now_utc().unix_timestamp() + 3600
        })))
        .mount(server).await;
}

fn setup(tmp: &tempfile::TempDir, server_url: &str) -> ApiClient {
    unsafe { std::env::set_var("GAUGE_CONFIG_DIR", tmp.path()) };
    gauge::keys::generate("alice").unwrap();
    ApiClient::from_config(&ClientConfig { server_url: server_url.trim_end_matches('/').into(), user_id: "alice".into() })
}

#[tokio::test]
async fn login_signs_nonce_and_caches_token() {
    let _g = env_lock();
    let tmp = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    mock_auth(&server).await;
    let api = setup(&tmp, &server.uri());

    let cache = api.login().await.unwrap();
    assert_eq!(cache.token, "test-token");
    assert!(tmp.path().join("token.json").exists());

    // the signature sent to /verify must verify against our key + the nonce
    let reqs: Vec<Request> = server.received_requests().await.unwrap();
    let verify_body: serde_json::Value =
        serde_json::from_slice(&reqs.iter().find(|r| r.url.path() == "/v1/auth/verify").unwrap().body).unwrap();
    let sig = gauge_auth::wire::b64_decode_flexible(verify_body["signature_b64"].as_str().unwrap()).unwrap();
    let kp: Keypair = gauge::keys::load_keypair("alice").unwrap();
    assert!(verify_signature(&kp.verifying_key(), &NONCE, &sig).is_ok());
    unsafe { std::env::remove_var("GAUGE_CONFIG_DIR") };
}

#[tokio::test]
async fn query_reauths_once_on_401() {
    let _g = env_lock();
    let tmp = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    mock_auth(&server).await;
    // first /v1/query call → 401; subsequent → 200
    Mock::given(method("POST")).and(path("/v1/query"))
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "code": "invalid_token", "message": "expired", "remediation": "run `gauge login`"
        })))
        .up_to_n_times(1)
        .mount(&server).await;
    Mock::given(method("POST")).and(path("/v1/query"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "rows": [{"count": 5}], "truncated": false, "elapsed_ms": 3
        })))
        .mount(&server).await;

    let api = setup(&tmp, &server.uri());
    let req: gauge_query::QueryRequest =
        serde_json::from_str(r#"{"measures":["count"],"time_range":{"last":"1d"}}"#).unwrap();
    let resp = api.query(&req).await.unwrap();
    assert_eq!(resp.rows[0]["count"], 5);
    unsafe { std::env::remove_var("GAUGE_CONFIG_DIR") };
}

#[tokio::test]
async fn api_error_envelope_is_surfaced() {
    let _g = env_lock();
    let tmp = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    mock_auth(&server).await;
    Mock::given(method("POST")).and(path("/v1/query"))
        .respond_with(ResponseTemplate::new(422).set_body_json(serde_json::json!({
            "code": "invalid_query", "message": "unknown field `nope`", "remediation": null
        })))
        .mount(&server).await;
    let api = setup(&tmp, &server.uri());
    let req: gauge_query::QueryRequest =
        serde_json::from_str(r#"{"measures":["count"],"time_range":{"last":"1d"}}"#).unwrap();
    let err = api.query(&req).await.unwrap_err();
    assert!(err.to_string().contains("invalid_query"));
    assert!(err.to_string().contains("unknown field"));
    unsafe { std::env::remove_var("GAUGE_CONFIG_DIR") };
}
