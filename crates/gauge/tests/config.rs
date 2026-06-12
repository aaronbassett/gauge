use std::sync::{Mutex, OnceLock};

/// Env vars are process-global; serialize tests that touch GAUGE_CONFIG_DIR.
pub fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}

#[test]
fn config_dir_honours_env_override() {
    let _g = env_lock();
    let tmp = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("GAUGE_CONFIG_DIR", tmp.path()) };
    assert_eq!(gauge::paths::config_dir().unwrap(), tmp.path());
    unsafe { std::env::remove_var("GAUGE_CONFIG_DIR") };
}

#[test]
fn config_loads_and_normalizes_server_url() {
    let _g = env_lock();
    let tmp = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("GAUGE_CONFIG_DIR", tmp.path()) };
    std::fs::write(
        tmp.path().join("config.toml"),
        "server_url = \"https://gauge-telemetry.fly.dev/\"\nuser_id = \"aaron\"\n",
    )
    .unwrap();
    let cfg = gauge::config::ClientConfig::load().unwrap();
    assert_eq!(cfg.server_url, "https://gauge-telemetry.fly.dev"); // trailing slash stripped
    assert_eq!(cfg.user_id, "aaron");
    unsafe { std::env::remove_var("GAUGE_CONFIG_DIR") };
}

#[test]
fn missing_config_names_the_path() {
    let _g = env_lock();
    let tmp = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("GAUGE_CONFIG_DIR", tmp.path()) };
    let err = gauge::config::ClientConfig::load().unwrap_err();
    assert!(err.to_string().contains("config.toml"));
    unsafe { std::env::remove_var("GAUGE_CONFIG_DIR") };
}
