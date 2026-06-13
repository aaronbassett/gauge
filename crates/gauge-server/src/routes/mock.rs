//! `POST /v1/mock` — demo data generator. Mounted always but gated on
//! `demo_mode` (ENABLE_DEMO_MODE=1); returns 404 when disabled. No auth.
//! Generates realistic, Gauge-profile-shaped synthetic events and stores them.
use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use gauge_events::profile::{HOST_ARCHS, OS_TYPES};
use serde_json::Value;
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::db::{self, GeneratedRow};
use crate::error::ApiError;
use crate::state::AppState;

const DEFAULT_COUNT: usize = 50;
const MAX_COUNT: usize = 100_000;
const DEFAULT_WINDOW_DAYS: i64 = 30;

const VERSIONS: &[&str] = &["0.6.0", "0.9.1", "1.0.0", "1.2.3", "2.0.0"];
const VERBS: &[&str] = &[
    "search", "install", "open", "export", "error", "sync", "login",
];
const SURFACES: &[&str] = &["cli", "mcp", "web", "tui"];
const LATENCY: &[&str] = &["<50ms", "50-200ms", "200-1000ms", ">1s"];

#[derive(Debug, Default, serde::Deserialize)]
struct MockRequest {
    count: Option<usize>,
    /// RFC3339; defaults to 30 days ago.
    start: Option<String>,
    /// RFC3339; defaults to now.
    end: Option<String>,
}

pub async fn mock(State(st): State<AppState>, body: Bytes) -> Result<Json<Value>, ApiError> {
    if !st.demo_mode {
        // Not available unless ENABLE_DEMO_MODE=1. 404 = "no such endpoint here".
        return Err(ApiError::not_found(
            "not_found",
            "endpoint not available (demo mode disabled)",
        ));
    }

    // Lenient body: empty or `{}` => all defaults.
    let req: MockRequest = if body.is_empty() {
        MockRequest::default()
    } else {
        serde_json::from_slice(&body).map_err(|e| {
            ApiError::bad_request("invalid_request", format!("invalid JSON body: {e}"))
        })?
    };

    let count = req.count.unwrap_or(DEFAULT_COUNT);
    if count > MAX_COUNT {
        return Err(ApiError::bad_request(
            "invalid_request",
            format!("count exceeds the {MAX_COUNT} cap"),
        ));
    }

    let now = OffsetDateTime::now_utc();
    let end = parse_opt(&req.end)?.unwrap_or(now);
    let start = parse_opt(&req.start)?.unwrap_or(now - Duration::days(DEFAULT_WINDOW_DAYS));
    if start >= end {
        return Err(ApiError::bad_request(
            "invalid_request",
            "start must be strictly before end",
        ));
    }

    let rows = generate(&st.allowlist, count, start, end);
    db::insert_generated(&st.pool, &rows).await.map_err(|e| {
        tracing::error!(kind = %e.to_string(), "mock insert failed");
        ApiError::service_unavailable(
            "db_unavailable",
            "could not persist mock events; retry later",
        )
    })?;
    tracing::info!(generated = count, "mock data generated");

    Ok(Json(serde_json::json!({
        "generated": count,
        "start": start.format(&Rfc3339).unwrap_or_default(),
        "end": end.format(&Rfc3339).unwrap_or_default(),
    })))
}

fn parse_opt(s: &Option<String>) -> Result<Option<OffsetDateTime>, ApiError> {
    match s {
        None => Ok(None),
        Some(v) => OffsetDateTime::parse(v, &Rfc3339).map(Some).map_err(|_| {
            ApiError::bad_request(
                "invalid_request",
                "start and end must be RFC3339 timestamps (e.g. 2026-06-01T00:00:00Z)",
            )
        }),
    }
}

struct Install {
    id: Uuid,
    app: String,
    app_version: String,
    os: String,
    arch: String,
}

/// Build `count` events drawn from a small pool of installs (so unique-install
/// counts are below the event count, like real telemetry). Apps come from the
/// allowlist; event names are `{app}.{verb}`; os/arch are valid profile values.
fn generate(
    allowlist: &[String],
    count: usize,
    start: OffsetDateTime,
    end: OffsetDateTime,
) -> Vec<GeneratedRow> {
    let apps: Vec<String> = if allowlist.is_empty() {
        vec!["demo-app".to_string()]
    } else {
        allowlist.to_vec()
    };
    let mut rng = Rng::from_entropy();

    let installs: Vec<Install> = (0..(count / 8).max(1))
        .map(|_| Install {
            id: Uuid::new_v4(),
            app: rng.pick(&apps).clone(),
            app_version: (*rng.pick(VERSIONS)).to_string(),
            os: (*rng.pick(OS_TYPES)).to_string(),
            arch: (*rng.pick(HOST_ARCHS)).to_string(),
        })
        .collect();
    let sessions: Vec<Uuid> = (0..(count / 3).max(1)).map(|_| Uuid::new_v4()).collect();

    let start_ns = start.unix_timestamp_nanos();
    let span = (end.unix_timestamp_nanos() - start_ns) as u128;

    (0..count)
        .map(|_| {
            let inst = rng.pick(&installs);
            let verb = *rng.pick(VERBS);
            let session = *rng.pick(&sessions);
            let surface = *rng.pick(SURFACES);
            let latency = *rng.pick(LATENCY);
            let success = rng.next_bool();
            let t_ns = start_ns + (rng.next_u128() % span) as i128;
            let time = OffsetDateTime::from_unix_timestamp_nanos(t_ns).unwrap_or(start);
            GeneratedRow {
                app: inst.app.clone(),
                app_version: inst.app_version.clone(),
                install_id: inst.id,
                session_id: session,
                os: inst.os.clone(),
                arch: inst.arch.clone(),
                event_name: format!("{}.{verb}", inst.app),
                time,
                attributes: serde_json::json!({
                    "surface": surface,
                    "latency_bucket": latency,
                    "success": success,
                }),
            }
        })
        .collect()
}

/// SplitMix64 — tiny, dependency-free PRNG. Demo data only (not cryptographic);
/// seeded from OS entropy via a v4 UUID.
struct Rng(u64);

impl Rng {
    fn from_entropy() -> Self {
        let bytes = Uuid::new_v4().into_bytes();
        Self(u64::from_le_bytes(
            bytes[0..8].try_into().expect("16-byte uuid"),
        ))
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn next_u128(&mut self) -> u128 {
        ((self.next_u64() as u128) << 64) | self.next_u64() as u128
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }

    fn next_bool(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }
}
