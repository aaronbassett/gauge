use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("invalid public key wire format (expected `ed25519:<base64>`)")]
    InvalidWireFormat,
    #[error("invalid key, nonce, or signature length")]
    InvalidLength,
    #[error("signature verification failed")]
    InvalidSignature,
    #[error("challenge not found or already used")]
    ChallengeNotFound,
    #[error("challenge expired")]
    ChallengeExpired,
    #[error("JWT secret must be at least 32 bytes")]
    SecretTooShort,
    #[error("jwt error: {0}")]
    Jwt(String),
    #[error("user store error: {0}")]
    UserStore(String),
    #[error("base64 decode error")]
    Base64,
}
