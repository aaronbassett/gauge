use std::sync::Arc;

use gauge_auth::{ChallengeStore, SigningSecret, UserStore};
use sqlx::PgPool;

use crate::config::Config;
use crate::middleware::rate_limit::Limiters;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub allowlist: Arc<Vec<String>>,
    pub users: Arc<UserStore>,
    pub challenges: Arc<ChallengeStore>,
    pub secret: Arc<SigningSecret>,
    pub limiters: Arc<Limiters>,
}

impl AppState {
    pub fn from_config(cfg: Config, pool: PgPool) -> Result<Self, String> {
        Ok(Self {
            pool,
            allowlist: Arc::new(cfg.app_allowlist),
            users: Arc::new(
                UserStore::from_toml_str(&cfg.user_store_toml).map_err(|e| e.to_string())?,
            ),
            challenges: Arc::new(ChallengeStore::new()),
            secret: Arc::new(cfg.jwt_secret),
            limiters: Arc::new(Limiters::new(
                cfg.rate_logs_per_min,
                cfg.rate_auth_per_min,
                cfg.rate_user_per_min,
            )),
        })
    }
}
