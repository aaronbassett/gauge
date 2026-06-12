use std::collections::HashMap;
use std::sync::Mutex;

use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::error::AuthError;
use crate::wire::NONCE_LEN;

/// FR from spec: challenge TTL is clamped to 60 seconds.
pub const CHALLENGE_TTL: Duration = Duration::seconds(60);

#[derive(Debug, Clone)]
pub struct Challenge {
    pub challenge_id: Uuid,
    pub user_id: String,
    pub nonce: [u8; NONCE_LEN],
    pub expires_at: OffsetDateTime,
}

#[derive(Default)]
pub struct ChallengeStore {
    inner: Mutex<HashMap<Uuid, Challenge>>,
}

impl ChallengeStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mint(&self, user_id: &str, now: OffsetDateTime) -> Challenge {
        let mut nonce = [0u8; NONCE_LEN];
        rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut nonce);
        let c = Challenge {
            challenge_id: Uuid::new_v4(),
            user_id: user_id.to_string(),
            nonce,
            expires_at: now + CHALLENGE_TTL,
        };
        self.inner.lock().unwrap().insert(c.challenge_id, c.clone());
        c
    }

    /// Removes the challenge regardless of outcome: expired consume attempts
    /// also burn the challenge (single-use either way).
    pub fn consume(&self, id: &Uuid, now: OffsetDateTime) -> Result<Challenge, AuthError> {
        let c = self
            .inner
            .lock()
            .unwrap()
            .remove(id)
            .ok_or(AuthError::ChallengeNotFound)?;
        if now > c.expires_at {
            return Err(AuthError::ChallengeExpired);
        }
        Ok(c)
    }

    pub fn purge_expired(&self, now: OffsetDateTime) {
        self.inner.lock().unwrap().retain(|_, c| c.expires_at >= now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    const T0: OffsetDateTime = datetime!(2026-06-12 10:00:00 UTC);

    #[test]
    fn mint_then_consume_within_ttl() {
        let store = ChallengeStore::new();
        let c = store.mint("alice", T0);
        assert_eq!(c.user_id, "alice");
        assert_eq!(c.expires_at, T0 + CHALLENGE_TTL);
        let consumed = store.consume(&c.challenge_id, T0 + time::Duration::seconds(30)).unwrap();
        assert_eq!(consumed.nonce, c.nonce);
    }

    #[test]
    fn consume_is_single_use() {
        let store = ChallengeStore::new();
        let c = store.mint("alice", T0);
        store.consume(&c.challenge_id, T0).unwrap();
        assert!(matches!(
            store.consume(&c.challenge_id, T0),
            Err(AuthError::ChallengeNotFound)
        ));
    }

    #[test]
    fn consume_after_expiry_fails_and_removes() {
        let store = ChallengeStore::new();
        let c = store.mint("alice", T0);
        let late = T0 + CHALLENGE_TTL + time::Duration::seconds(1);
        assert!(matches!(store.consume(&c.challenge_id, late), Err(AuthError::ChallengeExpired)));
        assert!(matches!(store.consume(&c.challenge_id, T0), Err(AuthError::ChallengeNotFound)));
    }

    #[test]
    fn purge_removes_only_expired() {
        let store = ChallengeStore::new();
        let old = store.mint("alice", T0 - time::Duration::minutes(5));
        let fresh = store.mint("bob", T0);
        store.purge_expired(T0);
        assert!(store.consume(&old.challenge_id, T0).is_err());
        assert!(store.consume(&fresh.challenge_id, T0).is_ok());
    }

    #[test]
    fn nonces_are_unique() {
        let store = ChallengeStore::new();
        assert_ne!(store.mint("a", T0).nonce, store.mint("a", T0).nonce);
    }
}
