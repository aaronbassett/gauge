use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::AuthError;
use crate::user::Role;

pub const TOKEN_TTL_SECS: i64 = 3600;

/// Opaque wrapper: prevents accidental Debug/log leak of the HS256 secret.
pub struct SigningSecret(Vec<u8>);

impl SigningSecret {
    pub fn new(bytes: impl Into<Vec<u8>>) -> Result<Self, AuthError> {
        let b = bytes.into();
        if b.len() < 32 {
            return Err(AuthError::SecretTooShort);
        }
        Ok(Self(b))
    }
}

impl std::fmt::Debug for SigningSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SigningSecret(redacted)")
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Claims {
    pub sub: String,
    pub iat: i64,
    pub exp: i64,
    pub role: Role,
    pub jti: String,
}

/// Returns (token, exp_unix_seconds).
pub fn mint_token(
    secret: &SigningSecret,
    user_id: &str,
    role: Role,
    now: OffsetDateTime,
) -> Result<(String, i64), AuthError> {
    let exp = now.unix_timestamp() + TOKEN_TTL_SECS;
    let claims = Claims {
        sub: user_id.to_string(),
        iat: now.unix_timestamp(),
        exp,
        role,
        jti: Uuid::new_v4().to_string(),
    };
    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(&secret.0),
    )
    .map_err(|e| AuthError::Jwt(e.to_string()))?;
    Ok((token, exp))
}

pub fn verify_token(secret: &SigningSecret, token: &str) -> Result<Claims, AuthError> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.leeway = 0;
    decode::<Claims>(token, &DecodingKey::from_secret(&secret.0), &validation)
        .map(|d| d.claims)
        .map_err(|e| AuthError::Jwt(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::user::Role;
    use time::macros::datetime;

    fn secret() -> SigningSecret {
        SigningSecret::new(vec![7u8; 32]).unwrap()
    }

    #[test]
    fn rejects_short_secret() {
        assert!(matches!(SigningSecret::new(vec![7u8; 31]), Err(AuthError::SecretTooShort)));
    }

    #[test]
    fn mint_verify_round_trip() {
        let now = OffsetDateTime::now_utc();
        let (token, exp) = mint_token(&secret(), "alice", Role::Admin, now).unwrap();
        assert_eq!(exp, now.unix_timestamp() + TOKEN_TTL_SECS);
        let claims = verify_token(&secret(), &token).unwrap();
        assert_eq!(claims.sub, "alice");
        assert_eq!(claims.role, Role::Admin);
        assert!(!claims.jti.is_empty());
    }

    #[test]
    fn tampered_token_fails() {
        let (token, _) = mint_token(&secret(), "alice", Role::Admin, OffsetDateTime::now_utc()).unwrap();
        let mut tampered = token.clone();
        tampered.push('x');
        assert!(verify_token(&secret(), &tampered).is_err());
    }

    #[test]
    fn wrong_secret_fails() {
        let (token, _) = mint_token(&secret(), "alice", Role::Admin, OffsetDateTime::now_utc()).unwrap();
        let other = SigningSecret::new(vec![9u8; 32]).unwrap();
        assert!(verify_token(&other, &token).is_err());
    }

    #[test]
    fn expired_token_fails() {
        let past = datetime!(2020-01-01 00:00:00 UTC);
        let (token, _) = mint_token(&secret(), "alice", Role::Viewer, past).unwrap();
        assert!(verify_token(&secret(), &token).is_err());
    }

    #[test]
    fn secret_debug_is_redacted() {
        assert_eq!(format!("{:?}", secret()), "SigningSecret(redacted)");
    }
}
