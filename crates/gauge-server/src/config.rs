use std::net::SocketAddr;

use gauge_auth::SigningSecret;

pub struct Config {
    pub listen_addr: SocketAddr,
    pub database_url: String,
    pub jwt_secret: SigningSecret,
    pub user_store_toml: String,
    pub app_allowlist: Vec<String>,
    pub rate_logs_per_min: u32,
    pub rate_auth_per_min: u32,
    pub rate_user_per_min: u32,
    /// `ENABLE_DEMO_MODE=1` exposes the unauthenticated `/v1/mock` data generator.
    pub enable_demo_mode: bool,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        fn req(k: &str) -> Result<String, String> {
            std::env::var(k).map_err(|_| format!("missing required env var {k}"))
        }
        fn opt_u32(k: &str, default: u32) -> Result<u32, String> {
            match std::env::var(k) {
                Ok(v) => v.parse().map_err(|_| format!("{k} must be an integer")),
                Err(_) => Ok(default),
            }
        }
        let listen_addr = std::env::var("GAUGE_LISTEN_ADDR")
            .unwrap_or_else(|_| "0.0.0.0:8080".into())
            .parse()
            .map_err(|e| format!("GAUGE_LISTEN_ADDR: {e}"))?;
        Ok(Self {
            listen_addr,
            database_url: req("DATABASE_URL")?,
            jwt_secret: SigningSecret::new(req("GAUGE_JWT_SECRET")?.into_bytes())
                .map_err(|e| e.to_string())?,
            user_store_toml: req("GAUGE_USER_STORE")?,
            app_allowlist: req("GAUGE_APP_ALLOWLIST")?
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            rate_logs_per_min: opt_u32("GAUGE_RATE_LOGS_PER_MIN", 60)?,
            rate_auth_per_min: opt_u32("GAUGE_RATE_AUTH_PER_MIN", 10)?,
            rate_user_per_min: opt_u32("GAUGE_RATE_USER_PER_MIN", 120)?,
            enable_demo_mode: std::env::var("ENABLE_DEMO_MODE").is_ok_and(|v| v == "1"),
        })
    }
}
