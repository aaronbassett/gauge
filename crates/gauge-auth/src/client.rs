use base64::Engine as _;
use base64::engine::general_purpose::STANDARD_NO_PAD;

use crate::error::AuthError;
use crate::keypair::Keypair;
use crate::wire::{NONCE_LEN, b64_decode_flexible};

/// Client half of the challenge/response: decode the server's nonce, sign it,
/// return the base64 signature for POST /v1/auth/verify.
pub fn sign_challenge(keypair: &Keypair, nonce_b64: &str) -> Result<String, AuthError> {
    let nonce = b64_decode_flexible(nonce_b64)?;
    if nonce.len() != NONCE_LEN {
        return Err(AuthError::InvalidLength);
    }
    Ok(STANDARD_NO_PAD.encode(keypair.sign(&nonce)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keypair::{Keypair, verify_signature};
    use base64::Engine as _;

    #[test]
    fn sign_challenge_produces_verifiable_signature() {
        let kp = Keypair::generate();
        let nonce = [42u8; 32];
        let nonce_b64 = base64::engine::general_purpose::STANDARD_NO_PAD.encode(nonce);
        let sig_b64 = sign_challenge(&kp, &nonce_b64).unwrap();
        let sig = crate::wire::b64_decode_flexible(&sig_b64).unwrap();
        assert!(verify_signature(&kp.verifying_key(), &nonce, &sig).is_ok());
    }

    #[test]
    fn sign_challenge_rejects_wrong_nonce_length() {
        let kp = Keypair::generate();
        let short = base64::engine::general_purpose::STANDARD_NO_PAD.encode([1u8; 8]);
        assert!(matches!(sign_challenge(&kp, &short), Err(AuthError::InvalidLength)));
    }
}
