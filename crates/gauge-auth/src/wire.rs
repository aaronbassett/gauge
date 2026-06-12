use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use ed25519_dalek::VerifyingKey;

use crate::error::AuthError;

pub const ED25519_WIRE_PREFIX: &str = "ed25519:";
pub const PUBLIC_KEY_LEN: usize = 32;
pub const SIGNATURE_LEN: usize = 64;
pub const NONCE_LEN: usize = 32;

/// Accepts both padded and unpadded standard base64 (mn-auth compatibility).
pub fn b64_decode_flexible(s: &str) -> Result<Vec<u8>, AuthError> {
    STANDARD
        .decode(s)
        .or_else(|_| STANDARD_NO_PAD.decode(s))
        .map_err(|_| AuthError::Base64)
}

pub fn parse_public_key_wire(wire: &str) -> Result<VerifyingKey, AuthError> {
    let b64 = wire
        .strip_prefix(ED25519_WIRE_PREFIX)
        .ok_or(AuthError::InvalidWireFormat)?;
    let bytes = b64_decode_flexible(b64)?;
    let arr: [u8; PUBLIC_KEY_LEN] = bytes.try_into().map_err(|_| AuthError::InvalidLength)?;
    VerifyingKey::from_bytes(&arr).map_err(|_| AuthError::InvalidWireFormat)
}

pub fn format_public_key_wire(key: &VerifyingKey) -> String {
    format!(
        "{ED25519_WIRE_PREFIX}{}",
        STANDARD_NO_PAD.encode(key.as_bytes())
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rejects_missing_prefix() {
        assert!(matches!(
            parse_public_key_wire("AAAA"),
            Err(AuthError::InvalidWireFormat)
        ));
    }

    #[test]
    fn parse_rejects_wrong_length() {
        assert!(matches!(
            parse_public_key_wire("ed25519:AAAA"),
            Err(AuthError::InvalidLength)
        ));
    }

    #[test]
    fn parse_rejects_bad_base64() {
        assert!(matches!(
            parse_public_key_wire("ed25519:!!!not-base64!!!"),
            Err(AuthError::Base64)
        ));
    }

    #[test]
    fn flexible_b64_accepts_padded_and_unpadded() {
        assert_eq!(b64_decode_flexible("aGk=").unwrap(), b"hi");
        assert_eq!(b64_decode_flexible("aGk").unwrap(), b"hi");
    }

    #[test]
    fn parse_rejects_invalid_curve_point() {
        // 32 bytes of the right length and valid base64, but not a decodable
        // Edwards point: this y-coordinate fails decompression in dalek, so the
        // length check passes and `VerifyingKey::from_bytes` is what rejects it.
        let mut bad = [0u8; 32];
        bad[0] = 0x02;
        bad[31] = 0x80;
        let wire = format!(
            "ed25519:{}",
            base64::engine::general_purpose::STANDARD_NO_PAD.encode(bad)
        );
        assert!(matches!(
            parse_public_key_wire(&wire),
            Err(AuthError::InvalidWireFormat)
        ));
    }
}
