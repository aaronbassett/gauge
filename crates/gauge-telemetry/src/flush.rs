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
    /// `threshold_bytes`. The size trigger fires once on crossing the threshold
    /// (the rising edge), not continuously while over it; `threshold_bytes == 0`
    /// disables the size trigger entirely (interval-only). `interval` is rounded
    /// up to the ~100ms tick granularity, which is the effective minimum drain
    /// cadence.
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
        Some(Flusher {
            stop,
            handle: Some(handle),
        })
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

/// The background drain loop for [`Flusher`]. Wakes every ~100ms — the
/// effective minimum drain cadence — to react to stop and to the queue size
/// threshold without waiting the full interval. The size trigger is
/// edge-triggered: it fires only on the rising edge (crossing the threshold),
/// not continuously while over it, so a small/zero threshold against a
/// non-draining queue (e.g. a down server) cannot hot-drain every tick.
/// `threshold_bytes == 0` disables the size trigger entirely.
fn run_loop(
    cfg: &SenderConfig,
    interval: Duration,
    threshold_bytes: u64,
    grace: Duration,
    mint: Option<SystemTime>,
    stop: &AtomicBool,
) {
    let tick = Duration::from_millis(100);
    let mut waited = Duration::ZERO;
    let mut was_over = false;
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(tick);
        waited += tick;
        let over = threshold_bytes > 0
            && std::fs::metadata(&cfg.queue_path)
                .map(|m| m.len() >= threshold_bytes)
                .unwrap_or(false);
        let crossed = over && !was_over; // rising edge only
        was_over = over;
        if waited >= interval || crossed {
            waited = Duration::ZERO;
            if !within_grace(mint, grace, SystemTime::now()) {
                let _ = drain(cfg);
            }
        }
    }
}

/// Build the (program, args) the DETACHED at-exit flush
/// ([`crate::client::Telemetry::spawn_detached_flush`]) will exec: the current
/// binary re-invoked with the app-registered flush args. This is NOT used by
/// the background [`Flusher`], which drains in-process. Pure for testing.
pub fn detached_command_parts(
    current_exe: &std::path::Path,
    flush_args: &[String],
) -> (String, Vec<String>) {
    (current_exe.display().to_string(), flush_args.to_vec())
}

#[cfg(test)]
mod detach_tests {
    use super::*;

    #[test]
    fn command_parts_reexec_current_exe_with_flush_args() {
        let exe = std::path::Path::new("/usr/local/bin/tome");
        let args = vec![
            "telemetry".to_string(),
            "flush".to_string(),
            "--quiet".to_string(),
        ];
        let (prog, got) = detached_command_parts(exe, &args);
        assert_eq!(prog, "/usr/local/bin/tome");
        assert_eq!(got, args);
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
            .ci(false)
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

    #[tokio::test]
    async fn interval_only_does_not_hot_drain() {
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
            .ci(false)
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
        // Huge interval + threshold 0 (size trigger disabled): nothing should
        // drain within a few ticks.
        let flusher = Flusher::start(&t, Duration::from_secs(3600), 0).unwrap();
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(
            !read_lines(&queue).unwrap().is_empty(),
            "queue must not drain: interval not elapsed and threshold disabled"
        );
        drop(flusher);
    }
}
