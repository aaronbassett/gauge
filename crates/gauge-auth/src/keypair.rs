use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::OsRng;

use crate::error::AuthError;
use crate::wire::{SIGNATURE_LEN, format_public_key_wire};

/// Holds the private signing key in memory. Never logged: no Debug impl.
pub struct Keypair(SigningKey);

impl Keypair {
    pub fn generate() -> Self {
        Self(SigningKey::generate(&mut OsRng))
    }

    pub fn from_seed(seed: &[u8; 32]) -> Self {
        Self(SigningKey::from_bytes(seed))
    }

    /// Returns the raw 32-byte private seed. Intended ONLY for one-time
    /// serialization to local key storage at enrollment — never call this on a
    /// request path, and never pass the result to `format!`/`tracing`/logging.
    /// The returned copy escapes `SigningKey`'s zeroize-on-drop, so the caller
    /// owns its lifetime. (Future hardening: return `zeroize::Zeroizing`.)
    pub fn seed(&self) -> [u8; 32] {
        self.0.to_bytes()
    }

    pub fn verifying_key(&self) -> VerifyingKey {
        self.0.verifying_key()
    }

    pub fn public_wire(&self) -> String {
        format_public_key_wire(&self.0.verifying_key())
    }

    pub fn sign(&self, msg: &[u8]) -> [u8; SIGNATURE_LEN] {
        self.0.sign(msg).to_bytes()
    }
}

pub fn verify_signature(key: &VerifyingKey, msg: &[u8], sig: &[u8]) -> Result<(), AuthError> {
    let sig: [u8; SIGNATURE_LEN] = sig.try_into().map_err(|_| AuthError::InvalidLength)?;
    let sig = Signature::from_bytes(&sig);
    key.verify(msg, &sig)
        .map_err(|_| AuthError::InvalidSignature)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::parse_public_key_wire;

    #[test]
    fn sign_verify_round_trip() {
        let kp = Keypair::generate();
        let sig = kp.sign(b"nonce-bytes");
        assert!(verify_signature(&kp.verifying_key(), b"nonce-bytes", &sig).is_ok());
    }

    #[test]
    fn tampered_message_fails() {
        let kp = Keypair::generate();
        let sig = kp.sign(b"nonce-bytes");
        assert!(matches!(
            verify_signature(&kp.verifying_key(), b"other-bytes", &sig),
            Err(AuthError::InvalidSignature)
        ));
    }

    #[test]
    fn wrong_key_fails() {
        let kp = Keypair::generate();
        let other = Keypair::generate();
        let sig = kp.sign(b"nonce-bytes");
        assert!(verify_signature(&other.verifying_key(), b"nonce-bytes", &sig).is_err());
    }

    #[test]
    fn public_wire_round_trips_through_parser() {
        let kp = Keypair::generate();
        let parsed = parse_public_key_wire(&kp.public_wire()).unwrap();
        assert_eq!(parsed.as_bytes(), kp.verifying_key().as_bytes());
    }

    #[test]
    fn seed_round_trip() {
        let kp = Keypair::generate();
        let restored = Keypair::from_seed(&kp.seed());
        assert_eq!(restored.public_wire(), kp.public_wire());
    }
}
