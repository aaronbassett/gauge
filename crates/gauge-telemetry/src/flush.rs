//! Out-of-band flush triggers for long-running processes: a background thread
//! that drains on an interval or when the queue grows past a threshold.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use gauge_events::sender::{SenderConfig, drain};

use crate::client::Telemetry;
use crate::consent::within_grace;

/// A running background flusher. Dropping it signals stop and joins the thread.
pub struct Flusher {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Flusher {
    /// Start a background flusher. Returns `None` if telemetry is disabled.
    /// Drains every `interval`, or sooner if the queue reaches or exceeds
    /// `threshold_bytes`. `interval` is rounded up to the ~100ms tick granularity.
    pub fn start(t: &Telemetry, interval: Duration, threshold_bytes: u64) -> Option<Flusher> {
        let inner = t.inner()?;
        let cfg = inner.cfg.clone();
        let grace = inner.grace;
        let mint = inner.mint_time;
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let handle = std::thread::spawn(move || {
            run_loop(&cfg, interval, threshold_bytes, grace, mint, &stop2);
        });
        Some(Flusher { stop, handle: Some(handle) })
    }
}

impl Drop for Flusher {
    fn drop(&mut self) {
        // Relaxed: lone signal flag with no dependent memory; Drop's join() provides
        // the shutdown happens-before.
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn run_loop(
    cfg: &SenderConfig,
    interval: Duration,
    threshold_bytes: u64,
    grace: Duration,
    mint: Option<SystemTime>,
    stop: &AtomicBool,
) {
    // Wake every 100ms to react to stop and to the queue size threshold
    // without waiting the full interval. 100ms is a floor: a sub-100ms
    // `interval` rounds up to this tick granularity rather than busy-spinning.
    let tick = Duration::from_millis(100);
    let mut waited = Duration::ZERO;
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(tick);
        waited += tick;
        let over_threshold = std::fs::metadata(&cfg.queue_path)
            .map(|m| m.len() >= threshold_bytes)
            .unwrap_or(false);
        if waited >= interval || over_threshold {
            waited = Duration::ZERO;
            if !within_grace(mint, grace, SystemTime::now()) {
                let _ = drain(cfg);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{CommandInvoked, Outcome, Surface};
    use gauge_events::sender::queue::read_lines;

    #[tokio::test]
    async fn background_flusher_drains_on_threshold() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/logs"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let tmp = tempfile::tempdir().unwrap();
        let t = Telemetry::builder()
            .app("mnm")
            .app_version("0.1.0")
            .endpoint(server.uri())
            .install_id_path(tmp.path().join("id"))
            .config_enabled(true)
            .runtime_enabled(true)
            .grace(Duration::ZERO)
            .build()
            .unwrap();
        t.emit(&CommandInvoked {
            command: "search".into(),
            duration_ms: 1,
            outcome: Outcome::Ok,
            surface: Surface::Mcp,
        });

        let queue = tmp.path().join("id.queue.jsonl");
        // threshold 1 byte → drains on the first tick
        let flusher = Flusher::start(&t, Duration::from_secs(60), 1).unwrap();

        // poll up to ~2s for the queue to drain
        let mut drained = false;
        for _ in 0..40 {
            if read_lines(&queue).unwrap().is_empty() {
                drained = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        drop(flusher);
        assert!(drained, "background flusher should drain the queue");
    }
}
