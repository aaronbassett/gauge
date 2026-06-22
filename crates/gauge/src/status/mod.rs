//! `gauge status` — client/server health + data overview.

pub mod art;

use std::io::Write as _;
use std::time::Duration;

use serde::Serialize;
use time::OffsetDateTime;

use crate::api::{ApiClient, TokenCache};
use crate::config::ClientConfig;
use crate::error::ClientError;
use crate::paths;
use crate::term;

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
        Self {
            present: false,
            valid: false,
            expires_at: None,
            expires_in_secs: None,
        }
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
            return StatusReport {
                gauge,
                client,
                server,
                data,
                overall: Overall::Unhealthy,
            };
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
    StatusReport {
        gauge,
        client,
        server,
        data,
        overall,
    }
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
    let Some(cache) = TokenCache::load() else {
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

// ---- Output ----------------------------------------------------------------

/// Emit the report: compact JSON when `json`, else the art+panel human view.
pub fn emit(report: &StatusReport, json: bool) {
    if json {
        let body = serde_json::to_string(report).unwrap_or_else(|_| "{}".to_string());
        println!("{body}");
    } else {
        emit_human(report);
    }
}

fn ok_mark() -> String {
    if term::color_enabled() {
        term::green("✓")
    } else {
        "[ok]".to_string()
    }
}
fn warn_mark() -> String {
    if term::color_enabled() {
        term::yellow("⚠")
    } else {
        "[warn]".to_string()
    }
}
fn fail_mark() -> String {
    if term::color_enabled() {
        term::red("✗")
    } else {
        "[fail]".to_string()
    }
}

fn token_line(t: &TokenStatus) -> String {
    if !t.present {
        return format!("{} none cached", fail_mark());
    }
    if t.valid {
        format!(
            "{} valid · expires in {}",
            ok_mark(),
            human_duration(t.expires_in_secs.unwrap_or(0))
        )
    } else {
        let expired = t.expires_in_secs.map(|s| s <= 0).unwrap_or(true);
        format!(
            "{} {}",
            warn_mark(),
            if expired { "expired" } else { "stale" }
        )
    }
}

fn reachable_line(s: &ServerStatus) -> String {
    if !s.reachable {
        return format!(
            "{} unreachable ({})",
            fail_mark(),
            s.error.as_deref().unwrap_or("no response")
        );
    }
    let db = if s.db_ready {
        format!("{} ready", ok_mark())
    } else {
        format!("{} not ready", fail_mark())
    };
    format!("{} ok · DB {}", ok_mark(), db)
}

fn overall_line(o: &Overall) -> String {
    match o {
        Overall::Healthy => format!("{} healthy", ok_mark()),
        Overall::Degraded => format!("{} degraded", warn_mark()),
        Overall::Unhealthy => format!("{} unhealthy", fail_mark()),
    }
}

fn collapse_home(path: &str) -> String {
    if let Ok(home) = std::env::var("HOME")
        && !home.is_empty()
        && path.starts_with(&home)
    {
        return path.replacen(&home, "~", 1);
    }
    path.to_string()
}

fn rel_from_rfc3339(s: &str) -> String {
    match OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339) {
        Ok(dt) => relative_time(
            dt.unix_timestamp(),
            OffsetDateTime::now_utc().unix_timestamp(),
        ),
        Err(_) => s.to_string(),
    }
}

// ---- Humanizers (deferred from Task 4 — land with their first consumer) -----

fn human_count(n: i64) -> String {
    let v = n as f64;
    if v < 1_000.0 {
        return n.to_string();
    }
    let trim = |x: f64, suf: &str| {
        if x.fract().abs() < 0.05 {
            format!("{}{suf}", x.round() as i64)
        } else {
            format!("{x:.1}{suf}")
        }
    };
    if v < 1_000_000.0 {
        trim(v / 1_000.0, "K")
    } else if v < 1_000_000_000.0 {
        trim(v / 1_000_000.0, "M")
    } else {
        trim(v / 1_000_000_000.0, "B")
    }
}

fn human_duration(secs: i64) -> String {
    let s = secs.max(0);
    if s < 60 {
        format!("{s}s")
    } else if s < 3_600 {
        format!("{}m", s / 60)
    } else if s < 86_400 {
        format!("{}h", s / 3_600)
    } else {
        format!("{}d", s / 86_400)
    }
}

fn relative_time(then: i64, now: i64) -> String {
    let d = (now - then).max(0);
    let plural = |n: i64| if n == 1 { "" } else { "s" };
    if d < 60 {
        "just now".to_string()
    } else if d < 3_600 {
        let m = d / 60;
        format!("{m} minute{} ago", plural(m))
    } else if d < 86_400 {
        let h = d / 3_600;
        format!("{h} hour{} ago", plural(h))
    } else {
        let days = d / 86_400;
        format!("{days} day{} ago", plural(days))
    }
}

/// The right-hand info panel as plain/colored lines (colour auto-off when not a
/// TTY, yielding the plain rendering used by tests and pipes).
fn human_panel(r: &StatusReport) -> Vec<String> {
    let key = |k: &str| term::label(&format!("{k:<12}"));
    let dash = "—".to_string();

    let mut lines = Vec::new();
    lines.push(term::bold(&format!("Gauge v{}", r.gauge)));
    lines.push(String::new());

    lines.push(term::dim("Client"));
    let config_val = if r.client.config_loaded {
        collapse_home(&r.client.config_path)
    } else {
        format!("missing ({})", collapse_home(&r.client.config_path))
    };
    lines.push(format!("{} {}", key("Config:"), config_val));
    lines.push(format!(
        "{} {}",
        key("User:"),
        if r.client.user_id.is_empty() {
            dash.clone()
        } else {
            r.client.user_id.clone()
        }
    ));
    lines.push(format!(
        "{} {}",
        key("Key:"),
        if r.client.key_present {
            format!("{} present", ok_mark())
        } else {
            format!("{} missing", fail_mark())
        }
    ));
    lines.push(format!("{} {}", key("Token:"), token_line(&r.client.token)));
    lines.push(String::new());

    lines.push(term::dim("Server"));
    lines.push(format!(
        "{} {}",
        key("Endpoint:"),
        if r.client.server_url.is_empty() {
            dash.clone()
        } else {
            r.client.server_url.clone()
        }
    ));
    lines.push(format!(
        "{} {}",
        key("Reachable:"),
        reachable_line(&r.server)
    ));
    if r.data.available {
        lines.push(format!(
            "{} {} · {} events",
            key("Apps:"),
            r.data.apps,
            human_count(r.data.total_events)
        ));
        lines.push(format!(
            "{} {}",
            key("Latest:"),
            r.data
                .last_event
                .as_deref()
                .map(rel_from_rfc3339)
                .unwrap_or(dash)
        ));
    } else {
        lines.push(format!(
            "{} {} {}",
            key("Data:"),
            warn_mark(),
            r.data.error.as_deref().unwrap_or("unavailable")
        ));
    }
    lines.push(String::new());

    lines.push(format!("{} {}", key("Overall:"), overall_line(&r.overall)));
    lines
}

fn emit_human(report: &StatusReport) {
    let mut out = std::io::stdout().lock();
    let panel = human_panel(report);

    // Sparkline colours reuse the same palette `gauge tui` resolves (default
    // tokyo-night, honouring a custom dashboard.toml). `load` never errors —
    // it falls back to the built-in default config.
    let accents = crate::tui::config::load().0.resolve_theme().palette.accents;

    const GAP: usize = 3;
    const PANEL_MIN: usize = 34;
    let show_art = term::stdout_is_tty() && term::term_width() >= art::ART_WIDTH + GAP + PANEL_MIN;

    if !show_art {
        for line in &panel {
            let _ = writeln!(out, "{line}");
        }
        return;
    }

    let art = art::sparkline(&accents);
    let blank = " ".repeat(art::ART_WIDTH);
    let gap = " ".repeat(GAP);
    let rows = art.len().max(panel.len());
    for i in 0..rows {
        let left = art.get(i).map(String::as_str).unwrap_or(&blank);
        let right = panel.get(i).map(String::as_str).unwrap_or("");
        if right.is_empty() {
            let _ = writeln!(out, "{}", left.trim_end());
        } else {
            let _ = writeln!(out, "{left}{gap}{right}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn srv(reachable: bool, db_ready: bool) -> ServerStatus {
        ServerStatus {
            endpoint: "u".into(),
            reachable,
            db_ready,
            error: None,
        }
    }
    fn data(available: bool) -> DataStatus {
        DataStatus {
            available,
            apps: 0,
            total_events: 0,
            last_event: None,
            per_app: vec![],
            error: None,
        }
    }

    #[test]
    fn classify_truth_table() {
        assert_eq!(
            classify(true, &srv(true, true), &data(true)),
            Overall::Healthy
        );
        assert_eq!(
            classify(true, &srv(true, true), &data(false)),
            Overall::Degraded
        );
        assert_eq!(
            classify(true, &srv(true, false), &data(true)),
            Overall::Unhealthy
        );
        assert_eq!(
            classify(true, &srv(false, false), &data(false)),
            Overall::Unhealthy
        );
        assert_eq!(
            classify(false, &srv(false, false), &data(false)),
            Overall::Unhealthy
        );
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

    fn healthy_report() -> StatusReport {
        StatusReport {
            gauge: "9.9.9".into(),
            client: ClientStatus {
                config_path: "/home/x/.config/gauge/config.toml".into(),
                config_loaded: true,
                server_url: "https://gauge.example".into(),
                user_id: "aaron".into(),
                key_present: true,
                token: TokenStatus {
                    present: true,
                    valid: true,
                    expires_at: Some(10),
                    expires_in_secs: Some(2_460),
                },
            },
            server: ServerStatus {
                endpoint: "https://gauge.example".into(),
                reachable: true,
                db_ready: true,
                error: None,
            },
            data: DataStatus {
                available: true,
                apps: 3,
                total_events: 1_200_000,
                last_event: Some("2026-06-22T10:25:00Z".into()),
                per_app: vec![],
                error: None,
            },
            overall: Overall::Healthy,
        }
    }

    #[test]
    fn json_has_expected_shape() {
        let json = serde_json::to_string(&healthy_report()).unwrap();
        assert!(json.contains("\"gauge\":\"9.9.9\""));
        assert!(json.contains("\"overall\":\"healthy\""));
        assert!(json.contains("\"reachable\":true"));
        assert!(json.contains("\"total_events\":1200000"));
    }

    #[test]
    fn human_panel_renders_sections_plainly() {
        // Colour off in the test process → plain `[ok]` glyphs, no ANSI.
        let panel = human_panel(&healthy_report());
        let joined = panel.join("\n");
        assert!(joined.contains("Gauge v9.9.9"));
        assert!(joined.contains("Client"));
        assert!(joined.contains("Server"));
        assert!(joined.contains("[ok] present"));
        assert!(joined.contains("3 · 1.2M events"));
        assert!(joined.contains("[ok] healthy"));
        assert!(!joined.contains('\x1b'), "no ANSI when colour disabled");
    }

    #[test]
    fn human_panel_shows_data_reason_when_unavailable() {
        let mut r = healthy_report();
        r.data = DataStatus::unavailable("unauthenticated");
        r.overall = Overall::Degraded;
        let joined = human_panel(&r).join("\n");
        assert!(joined.contains("Data:"));
        assert!(joined.contains("unauthenticated"));
        assert!(joined.contains("[warn] degraded"));
    }

    // Deferred from Task 4: the humanizers land here with their first
    // production consumer (the renderer), so they never trip `dead_code`.
    #[test]
    fn humanizers() {
        assert_eq!(human_count(999), "999");
        assert_eq!(human_count(1_000), "1K");
        assert_eq!(human_count(1_200_000), "1.2M");
        assert_eq!(human_count(3_000_000_000), "3B");
        assert_eq!(human_duration(30), "30s");
        assert_eq!(human_duration(2_460), "41m");
        assert_eq!(human_duration(7_200), "2h");
        assert_eq!(human_duration(172_800), "2d");
        assert_eq!(relative_time(1_000, 1_000), "just now");
        assert_eq!(relative_time(1_000, 1_000 + 3_600), "1 hour ago");
    }
}
