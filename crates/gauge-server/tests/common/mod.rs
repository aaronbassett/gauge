use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use gauge_auth::{ChallengeStore, Keypair, SigningSecret, UserStore};
use gauge_server::state::AppState;
use sqlx::PgPool;
use tower::ServiceExt as _;

pub const TEST_SECRET: [u8; 32] = [7u8; 32];

/// AppState with one registered admin user ("alice") and apps tome + midnight-manual allowlisted.
pub fn test_state(pool: PgPool) -> (AppState, Keypair) {
    let kp = Keypair::generate();
    let toml = format!(
        "schema_version = 1\n\n[[users]]\nuser_id = \"alice\"\nrole = \"admin\"\npublic_key = \"{}\"\n",
        kp.public_wire()
    );
    let state = AppState {
        pool,
        allowlist: Arc::new(vec!["tome".into(), "midnight-manual".into()]),
        users: Arc::new(UserStore::from_toml_str(&toml).unwrap()),
        challenges: Arc::new(ChallengeStore::new()),
        secret: Arc::new(SigningSecret::new(TEST_SECRET.to_vec()).unwrap()),
    };
    (state, kp)
}

pub async fn send_json(
    app: &Router,
    method: &str,
    uri: &str,
    body: Option<serde_json::Value>,
    bearer: Option<&str>,
) -> (StatusCode, serde_json::Value) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(b) = bearer {
        req = req.header(header::AUTHORIZATION, format!("Bearer {b}"));
    }
    let req = match body {
        Some(v) => req
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => req.body(Body::empty()).unwrap(),
    };
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::String(
            String::from_utf8_lossy(&bytes).into_owned(),
        ))
    };
    (status, json)
}
