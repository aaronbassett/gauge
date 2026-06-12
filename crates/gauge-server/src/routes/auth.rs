use axum::Json;
use axum::extract::State;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use gauge_auth::protocol::{ChallengeRequest, ChallengeResponse, VerifyRequest, VerifyResponse};
use gauge_auth::wire::{b64_decode_flexible, parse_public_key_wire};
use gauge_auth::{AuthError, mint_token, verify_signature};
use time::OffsetDateTime;

use crate::error::ApiError;
use crate::state::AppState;

pub async fn challenge(
    State(st): State<AppState>,
    Json(req): Json<ChallengeRequest>,
) -> Result<Json<ChallengeResponse>, ApiError> {
    if req.user_id.trim().is_empty() {
        return Err(ApiError::bad_request(
            "invalid_request",
            "user_id must not be empty",
        ));
    }
    let now = OffsetDateTime::now_utc();
    st.challenges.purge_expired(now);
    if st.users.get(&req.user_id).is_none() {
        // same body as a consumed challenge: prevents user enumeration
        return Err(ApiError::not_found(
            "not_found",
            "unknown user or challenge",
        ));
    }
    let c = st.challenges.mint(&req.user_id, now);
    Ok(Json(ChallengeResponse {
        challenge_id: c.challenge_id,
        nonce_b64: STANDARD_NO_PAD.encode(c.nonce),
        expires_in_s: 60,
    }))
}

pub async fn verify(
    State(st): State<AppState>,
    Json(req): Json<VerifyRequest>,
) -> Result<Json<VerifyResponse>, ApiError> {
    let now = OffsetDateTime::now_utc();
    let challenge = st
        .challenges
        .consume(&req.challenge_id, now)
        .map_err(|e| match e {
            AuthError::ChallengeExpired => {
                ApiError::unauthorized("challenge_expired", "challenge expired")
                    .with_remediation("request a new challenge and sign it within 60 seconds")
            }
            _ => ApiError::not_found("not_found", "unknown user or challenge"),
        })?;
    let user = st
        .users
        .get(&challenge.user_id)
        .ok_or_else(|| ApiError::not_found("not_found", "unknown user or challenge"))?;
    let key = parse_public_key_wire(&user.public_key).map_err(|_| {
        ApiError::service_unavailable("bad_user_store", "stored public key is invalid")
    })?;
    let sig = b64_decode_flexible(&req.signature_b64).map_err(|_| {
        ApiError::bad_request("invalid_request", "signature_b64 is not valid base64")
    })?;
    verify_signature(&key, &challenge.nonce, &sig).map_err(|_| {
        ApiError::forbidden("invalid_signature", "signature verification failed")
            .with_remediation("check that the local keypair matches the registered public key")
    })?;
    let (token, expires_at) = mint_token(&st.secret, &user.user_id, user.role, now)
        .map_err(|_| ApiError::service_unavailable("jwt_error", "could not mint token"))?;
    tracing::info!(user = %user.user_id, "admin token issued");
    Ok(Json(VerifyResponse {
        token,
        user_id: user.user_id.clone(),
        expires_at,
    }))
}
