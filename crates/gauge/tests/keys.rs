// Integration tests are separate binaries; the env_lock helper is duplicated
// from tests/config.rs deliberately.
use std::sync::{Mutex, OnceLock};
fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}

#[test]
fn generate_writes_0600_key_and_returns_wire_pubkey() {
    let _g = env_lock();
    let tmp = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("GAUGE_CONFIG_DIR", tmp.path()) };

    let wire = gauge::keys::generate("alice").unwrap();
    assert!(wire.starts_with("ed25519:"));
    gauge_auth::wire::parse_public_key_wire(&wire).unwrap();

    let path = tmp.path().join("alice.private");
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);

    // load_keypair restores the same public key
    let kp = gauge::keys::load_keypair("alice").unwrap();
    assert_eq!(kp.public_wire(), wire);

    // refuses overwrite
    assert!(matches!(
        gauge::keys::generate("alice"),
        Err(gauge::error::ClientError::KeyExists(_))
    ));
    unsafe { std::env::remove_var("GAUGE_CONFIG_DIR") };
}
