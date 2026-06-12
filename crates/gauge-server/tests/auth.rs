mod common;

use axum::http::StatusCode;
use base64::Engine as _;
use gauge_auth::{SigningSecret, sign_challenge, verify_token};
use gauge_server::app::build_router;
use sqlx::PgPool;

#[sqlx::test]
async fn full_handshake_issues_valid_jwt(pool: PgPool) {
    let (state, kp) = common::test_state(pool);
    let app = build_router(state);

    let (status, ch) = common::send_json(
        &app, "POST", "/v1/auth/challenge",
        Some(serde_json::json!({"user_id": "alice"})), None,
    ).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ch["expires_in_s"], 60);

    let sig = sign_challenge(&kp, ch["nonce_b64"].as_str().unwrap()).unwrap();
    let (status, v) = common::send_json(
        &app, "POST", "/v1/auth/verify",
        Some(serde_json::json!({"challenge_id": ch["challenge_id"], "signature_b64": sig})), None,
    ).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(v["user_id"], "alice");

    let secret = SigningSecret::new(common::TEST_SECRET.to_vec()).unwrap();
    let claims = verify_token(&secret, v["token"].as_str().unwrap()).unwrap();
    assert_eq!(claims.sub, "alice");
    assert_eq!(claims.exp, v["expires_at"].as_i64().unwrap());
}

#[sqlx::test]
async fn unknown_user_is_404(pool: PgPool) {
    let (state, _kp) = common::test_state(pool);
    let app = build_router(state);
    let (status, _) = common::send_json(
        &app, "POST", "/v1/auth/challenge",
        Some(serde_json::json!({"user_id": "mallory"})), None,
    ).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn wrong_signature_is_403(pool: PgPool) {
    let (state, _kp) = common::test_state(pool);
    let app = build_router(state);
    let (_, ch) = common::send_json(
        &app, "POST", "/v1/auth/challenge",
        Some(serde_json::json!({"user_id": "alice"})), None,
    ).await;
    let bogus = base64::engine::general_purpose::STANDARD_NO_PAD.encode([0u8; 64]);
    let (status, resp) = common::send_json(
        &app, "POST", "/v1/auth/verify",
        Some(serde_json::json!({"challenge_id": ch["challenge_id"], "signature_b64": bogus})), None,
    ).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(resp["code"], "invalid_signature");
}

#[sqlx::test]
async fn challenge_is_single_use(pool: PgPool) {
    let (state, kp) = common::test_state(pool);
    let app = build_router(state);
    let (_, ch) = common::send_json(
        &app, "POST", "/v1/auth/challenge",
        Some(serde_json::json!({"user_id": "alice"})), None,
    ).await;
    let sig = sign_challenge(&kp, ch["nonce_b64"].as_str().unwrap()).unwrap();
    let body = serde_json::json!({"challenge_id": ch["challenge_id"], "signature_b64": sig});
    let (first, _) = common::send_json(&app, "POST", "/v1/auth/verify", Some(body.clone()), None).await;
    assert_eq!(first, StatusCode::OK);
    let (second, _) = common::send_json(&app, "POST", "/v1/auth/verify", Some(body), None).await;
    assert_eq!(second, StatusCode::NOT_FOUND);
}
