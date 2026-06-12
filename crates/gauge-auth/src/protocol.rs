#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChallengeRequest {
    pub user_id: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChallengeResponse {
    pub challenge_id: uuid::Uuid,
    pub nonce_b64: String,
    pub expires_in_s: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VerifyRequest {
    pub challenge_id: uuid::Uuid,
    pub signature_b64: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VerifyResponse {
    pub token: String,
    pub user_id: String,
    /// Unix seconds.
    pub expires_at: i64,
}
