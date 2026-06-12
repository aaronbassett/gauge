//! Hand-rolled keyed token buckets. IPs live ONLY here, in memory —
//! never on disk, never on event rows (spec privacy guarantee).

use std::collections::HashMap;
use std::hash::Hash;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Mutex;
use std::time::Instant;

use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::middleware::bearer::AuthContext;
use crate::state::AppState;

pub struct KeyedLimiter<K: Eq + Hash> {
    capacity: f64,
    refill_per_sec: f64,
    buckets: Mutex<HashMap<K, (f64, Instant)>>,
}

impl<K: Eq + Hash> KeyedLimiter<K> {
    pub fn new(per_min: u32, burst: u32) -> Self {
        Self {
            capacity: burst as f64,
            refill_per_sec: per_min as f64 / 60.0,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// Ok(()) consumes one token; Err(retry_after_secs) when exhausted.
    pub fn check(&self, key: K, now: Instant) -> Result<(), u64> {
        let mut map = self.buckets.lock().unwrap();
        let (tokens, last) = map.remove(&key).unwrap_or((self.capacity, now));
        let tokens = (tokens + now.duration_since(last).as_secs_f64() * self.refill_per_sec)
            .min(self.capacity);
        if tokens >= 1.0 {
            map.insert(key, (tokens - 1.0, now));
            Ok(())
        } else {
            let retry = ((1.0 - tokens) / self.refill_per_sec).ceil() as u64;
            map.insert(key, (tokens, now));
            Err(retry.max(1))
        }
    }
}

pub struct Limiters {
    pub logs: KeyedLimiter<IpAddr>,
    pub auth: KeyedLimiter<IpAddr>,
    pub user: KeyedLimiter<String>,
}

impl Limiters {
    /// burst = 2x for ingest (sender flushes are bursty), 1x elsewhere.
    pub fn new(logs_per_min: u32, auth_per_min: u32, user_per_min: u32) -> Self {
        Self {
            logs: KeyedLimiter::new(logs_per_min, logs_per_min * 2),
            auth: KeyedLimiter::new(auth_per_min, auth_per_min),
            user: KeyedLimiter::new(user_per_min, user_per_min),
        }
    }
}

/// Fly terminates TLS and sets Fly-Client-IP. Absent (local/tests) → loopback.
pub fn client_ip(headers: &HeaderMap) -> IpAddr {
    headers
        .get("Fly-Client-IP")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST))
}

fn too_many(retry_after: u64) -> Response {
    let body = serde_json::json!({
        "code": "rate_limited",
        "message": "rate limit exceeded",
        "remediation": format!("retry after {retry_after}s"),
    });
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(header::RETRY_AFTER, retry_after.to_string())],
        axum::Json(body),
    )
        .into_response()
}

pub async fn limit_logs(State(st): State<AppState>, req: Request, next: Next) -> Response {
    match st.limiters.logs.check(client_ip(req.headers()), Instant::now()) {
        Ok(()) => next.run(req).await,
        Err(r) => too_many(r),
    }
}

pub async fn limit_auth(State(st): State<AppState>, req: Request, next: Next) -> Response {
    match st.limiters.auth.check(client_ip(req.headers()), Instant::now()) {
        Ok(()) => next.run(req).await,
        Err(r) => too_many(r),
    }
}

/// Must run AFTER require_bearer (reads AuthContext from extensions).
pub async fn limit_user(State(st): State<AppState>, req: Request, next: Next) -> Response {
    let sub = req
        .extensions()
        .get::<AuthContext>()
        .map(|c| c.sub.clone())
        .unwrap_or_default();
    match st.limiters.user.check(sub, Instant::now()) {
        Ok(()) => next.run(req).await,
        Err(r) => too_many(r),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};
    use std::time::{Duration, Instant};

    const IP: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));

    #[test]
    fn allows_burst_then_blocks() {
        let l = KeyedLimiter::new(60, 2); // 1/sec refill, burst 2
        let t0 = Instant::now();
        assert!(l.check(IP, t0).is_ok());
        assert!(l.check(IP, t0).is_ok());
        let retry = l.check(IP, t0).unwrap_err();
        assert!(retry >= 1);
    }

    #[test]
    fn refills_over_time() {
        let l = KeyedLimiter::new(60, 1);
        let t0 = Instant::now();
        assert!(l.check(IP, t0).is_ok());
        assert!(l.check(IP, t0).is_err());
        assert!(l.check(IP, t0 + Duration::from_secs(2)).is_ok());
    }

    #[test]
    fn keys_are_independent() {
        let l = KeyedLimiter::new(60, 1);
        let t0 = Instant::now();
        let other = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2));
        assert!(l.check(IP, t0).is_ok());
        assert!(l.check(other, t0).is_ok());
    }
}
