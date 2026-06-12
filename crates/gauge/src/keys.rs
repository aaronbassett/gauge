use std::io::Write as _;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use gauge_auth::wire::b64_decode_flexible;
use gauge_auth::{AuthError, Keypair};

use crate::error::ClientError;
use crate::paths;

/// Generates a keypair, stores the seed (base64, mode 0600), returns the
/// public key wire form for registration in the server's users.toml.
pub fn generate(user_id: &str) -> Result<String, ClientError> {
    let dir = paths::config_dir()?;
    std::fs::create_dir_all(&dir)?;
    let path = paths::key_path(user_id)?;
    if path.exists() {
        return Err(ClientError::KeyExists(path));
    }
    let kp = Keypair::generate();
    let seed_b64 = STANDARD_NO_PAD.encode(kp.seed());
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        opts.mode(0o600);
    }
    let mut f = opts.open(&path)?;
    f.write_all(seed_b64.as_bytes())?;
    Ok(kp.public_wire())
}

pub fn load_keypair(user_id: &str) -> Result<Keypair, ClientError> {
    let path = paths::key_path(user_id)?;
    let b64 = std::fs::read_to_string(&path).map_err(|_| ClientError::KeyMissing(path))?;
    let bytes = b64_decode_flexible(b64.trim())?;
    let seed: [u8; 32] = bytes
        .try_into()
        .map_err(|_| ClientError::Auth(AuthError::InvalidLength))?;
    Ok(Keypair::from_seed(&seed))
}
