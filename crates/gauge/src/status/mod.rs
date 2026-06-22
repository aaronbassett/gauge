//! `gauge status` — client/server health + data overview.

pub mod art;

use std::time::Duration;

use serde::Serialize;
use time::OffsetDateTime;

use crate::api::{ApiClient, TokenCache};
use crate::config::ClientConfig;
use crate::error::ClientError;
use crate::paths;

// ---- Report data model -----------------------------------------------------

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Overall {
    Healthy,
    Degraded,
    Unhealthy,
}

impl Overall {
    /// 0 when healthy; 1 for degraded/unhealthy (Tome parity).
    pub fn exit_code(&self) -> i32 {
        match self {
            Overall::Healthy => 0,
            Overall::Degraded | Overall::Unhealthy => 1,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct TokenStatus {
    pub present: bool,
    pub valid: bool,
    pub expires_at: Option<i64>,
    pub expires_in_secs: Option<i64>,
}

impl TokenStatus {
    fn absent() -> Self {
        Self { present: false, valid: false, expires_at: None, expires_in_secs: None }
    }
}

#[derive(Debug, Serialize)]
pub struct ClientStatus {
    pub config_path: String,
    pub config_loaded: bool,
    pub server_url: String,
    pub user_id: String,
    pub key_present: bool,
    pub token: TokenStatus,
}

#[derive(Debug, Serialize)]
pub struct ServerStatus {
    pub endpoint: String,
    pub reachable: bool,
    pub db_ready: bool,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AppData {
    pub app: String,
    pub total_events: i64,
    pub last_event: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DataStatus {
    pub available: bool,
    pub apps: usize,
    pub total_events: i64,
    pub last_event: Option<String>,
    pub per_app: Vec<AppData>,
    pub error: Option<String>,
}

impl DataStatus {
    fn unavailable(reason: &str) -> Self {
        Self {
            available: false,
            apps: 0,
            total_events: 0,
            last_event: None,
            per_app: vec![],
            error: Some(reason.to_string()),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct StatusReport {
    pub gauge: String,
    pub client: ClientStatus,
    pub server: ServerStatus,
    pub data: DataStatus,
    pub overall: Overall,
}

// ---- Assembly --------------------------------------------------------------

/// Status probe request timeout — short so the command stays responsive when
/// the server is unreachable (the default `ApiClient` timeout is 10s).
const PROBE_TIMEOUT: Duration = Duration::from_secs(4);

/// Build the report from local state + best-effort network probes. Infallible
/// at the report level: every failure becomes a field. `config` is the result
/// of [`ClientConfig::load`]; when it is `Err`, no `ApiClient` is built and the
/// network sections short-circuit to unreachable/unavailable.
pub async fn assemble_report(config: Result<ClientConfig, ClientError>) -> StatusReport {
    let gauge = env!("CARGO_PKG_VERSION").to_string();
    let config_path = paths::config_path()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let now = OffsetDateTime::now_utc().unix_timestamp();

    let cfg = match config {
        Ok(c) => c,
        Err(_) => {
            let client = ClientStatus {
                config_path,
                config_loaded: false,
                server_url: String::new(),
                user_id: String::new(),
                key_present: false,
                token: TokenStatus::absent(),
            };
            let server = ServerStatus {
                endpoint: String::new(),
                reachable: false,
                db_ready: false,
                error: Some("client not configured".into()),
            };
            let data = DataStatus::unavailable("client not configured");
            return StatusReport { gauge, client, server, data, overall: Overall::Unhealthy };
        }
    };

    let key_present = paths::key_path(&cfg.user_id)
        .map(|p| p.exists())
        .unwrap_or(false);
    let token = token_status(&cfg.user_id, now);

    let api = ApiClient::from_config_with_timeout(&cfg, PROBE_TIMEOUT);
    let (reachable, db_ready, server_err) = probe_server(&api).await;
    let server = ServerStatus {
        endpoint: cfg.server_url.clone(),
        reachable,
        db_ready,
        error: server_err,
    };

    let data = if reachable {
        match api.meta().await {
            Ok(meta) => data_from_meta(&meta),
            Err(e) => DataStatus::unavailable(&e.to_string()),
        }
    } else {
        DataStatus::unavailable("server unreachable")
    };

    let client = ClientStatus {
        config_path,
        config_loaded: true,
        server_url: cfg.server_url.clone(),
        user_id: cfg.user_id.clone(),
        key_present,
        token,
    };
    let overall = classify(true, &server, &data);
    StatusReport { gauge, client, server, data, overall }
}

/// `healthz` then `readyz`. Returns `(reachable, db_ready, error)`.
async fn probe_server(api: &ApiClient) -> (bool, bool, Option<String>) {
    match api.healthz().await {
        Ok(()) => match api.readyz().await {
            Ok(()) => (true, true, None),
            Err(e) => (true, false, Some(e.to_string())),
        },
        Err(e) => (false, false, Some(e.to_string())),
    }
}

fn token_status(user_id: &str, now: i64) -> TokenStatus {
    let Some(cache) = read_token_cache() else {
        return TokenStatus::absent();
    };
    let valid = cache.user_id == user_id && cache.expires_at > now;
    TokenStatus {
        present: true,
        valid,
        expires_at: Some(cache.expires_at),
        expires_in_secs: Some(cache.expires_at - now),
    }
}

/// Read `token.json` directly (the struct is public + `Deserialize`); never
/// mints a token, so inspecting status performs no auth I/O.
fn read_token_cache() -> Option<TokenCache> {
    let path = paths::token_path().ok()?;
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn data_from_meta(meta: &gauge_query::MetaResponse) -> DataStatus {
    let per_app: Vec<AppData> = meta
        .apps
        .iter()
        .map(|a| AppData {
            app: a.app.clone(),
            total_events: a.total_events,
            last_event: a.last_event.clone(),
        })
        .collect();
    let total_events = per_app.iter().map(|a| a.total_events).sum();
    // RFC3339 strings sort lexicographically for a fixed offset (server emits
    // `Z`), so `max` gives the most recent.
    let last_event = meta.apps.iter().filter_map(|a| a.last_event.clone()).max();
    DataStatus {
        available: true,
        apps: per_app.len(),
        total_events,
        last_event,
        per_app,
        error: None,
    }
}

fn classify(config_loaded: bool, server: &ServerStatus, data: &DataStatus) -> Overall {
    if !config_loaded || !server.reachable || !server.db_ready {
        return Overall::Unhealthy;
    }
    if !data.available {
        return Overall::Degraded;
    }
    Overall::Healthy
}

#[cfg(test)]
mod tests {
    use super::*;

    fn srv(reachable: bool, db_ready: bool) -> ServerStatus {
        ServerStatus { endpoint: "u".into(), reachable, db_ready, error: None }
    }
    fn data(available: bool) -> DataStatus {
        DataStatus { available, apps: 0, total_events: 0, last_event: None, per_app: vec![], error: None }
    }

    #[test]
    fn classify_truth_table() {
        assert_eq!(classify(true, &srv(true, true), &data(true)), Overall::Healthy);
        assert_eq!(classify(true, &srv(true, true), &data(false)), Overall::Degraded);
        assert_eq!(classify(true, &srv(true, false), &data(true)), Overall::Unhealthy);
        assert_eq!(classify(true, &srv(false, false), &data(false)), Overall::Unhealthy);
        assert_eq!(classify(false, &srv(false, false), &data(false)), Overall::Unhealthy);
    }

    #[test]
    fn exit_code_matches_tome_parity() {
        assert_eq!(Overall::Healthy.exit_code(), 0);
        assert_eq!(Overall::Degraded.exit_code(), 1);
        assert_eq!(Overall::Unhealthy.exit_code(), 1);
    }

    #[tokio::test]
    async fn unconfigured_is_unhealthy() {
        let report = assemble_report(Err(crate::error::ClientError::NoConfigDir)).await;
        assert_eq!(report.overall, Overall::Unhealthy);
        assert!(!report.client.config_loaded);
        assert!(!report.server.reachable);
        assert!(!report.data.available);
    }
}
