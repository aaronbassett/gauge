//! The `Telemetry` handle and its builder. Built once at startup; a disabled
//! handle is a cheap no-op so call sites stay ergonomic.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use uuid::Uuid;

use gauge_events::sender::{SenderConfig, enqueue};

use crate::consent::{self, ConsentInputs, GLOBAL_DISABLE_VAR};
use crate::env::{self, EnvAttributes};
use crate::event::{Event, to_attributes};
use crate::identity;

/// Default first-run grace period before the first flush.
pub const DEFAULT_GRACE: Duration = Duration::from_secs(600); // 10 minutes

pub(crate) struct Inner {
    pub cfg: SenderConfig,
    pub grace: Duration,
    pub mint_time: Option<SystemTime>,
    pub env: EnvAttributes,
    pub flush_args: Vec<String>,
    pub install_id_path: PathBuf,
}

/// The telemetry handle. `None` inner = disabled (consent resolved to off).
pub struct Telemetry(pub(crate) Option<Inner>);

impl Telemetry {
    pub fn builder() -> Builder {
        Builder::default()
    }

    pub(crate) fn inner(&self) -> Option<&Inner> {
        self.0.as_ref()
    }

    /// True if telemetry is enabled (consent on). A disabled handle no-ops.
    pub fn is_enabled(&self) -> bool {
        self.0.is_some()
    }

    /// The captured environment snapshot, for `Install`/`Heartbeat` events.
    pub fn env(&self) -> EnvAttributes {
        self.0.as_ref().map(|i| i.env.clone()).unwrap_or_default()
    }

    /// Append one event to the disk queue. No network, never fails the caller.
    pub fn emit<E: Event>(&self, event: &E) {
        let Some(inner) = &self.0 else {
            return;
        };
        let attrs = match to_attributes(event) {
            Ok(a) => a,
            Err(e) => {
                debug_assert!(false, "telemetry event `{}` rejected: {e}", event.name());
                return;
            }
        };
        let full = format!("{}.{}", inner.cfg.app, event.name());
        let _ = enqueue(&inner.cfg, &full, attrs); // best-effort, non-fatal
    }

    /// Best-effort synchronous flush, capped by `timeout` of wall-clock. Runs
    /// the blocking drain on a worker thread so it is safe to call from async
    /// contexts via `spawn_blocking`. No-op while inside the first-run grace.
    pub fn flush_blocking(&self, timeout: Duration) {
        let Some(inner) = &self.0 else {
            return;
        };
        if consent::within_grace(inner.mint_time, inner.grace, SystemTime::now()) {
            return;
        }
        let cfg = inner.cfg.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(gauge_events::sender::drain(&cfg));
        });
        let _ = rx.recv_timeout(timeout); // ignore result and timeout; best-effort
    }

    /// Regenerate the install UUID and clear the queue.
    pub fn reset(&self) -> std::io::Result<()> {
        let Some(inner) = &self.0 else {
            return Ok(());
        };
        identity::reset(&inner.install_id_path)?;
        let _ = std::fs::remove_file(&inner.cfg.queue_path);
        Ok(())
    }
}

#[derive(Default)]
pub struct Builder {
    app: Option<String>,
    app_version: Option<String>,
    endpoint: Option<String>,
    install_id_path: Option<PathBuf>,
    queue_path: Option<PathBuf>,
    app_env_var: Option<String>,
    config_enabled: bool,
    runtime_enabled: bool,
    grace: Option<Duration>,
    flush_args: Vec<String>,
    accel: Option<String>,
}

impl Builder {
    pub fn app(mut self, v: impl Into<String>) -> Self {
        self.app = Some(v.into());
        self
    }
    pub fn app_version(mut self, v: impl Into<String>) -> Self {
        self.app_version = Some(v.into());
        self
    }
    pub fn endpoint(mut self, v: impl Into<String>) -> Self {
        self.endpoint = Some(v.into());
        self
    }
    pub fn install_id_path(mut self, p: impl Into<PathBuf>) -> Self {
        self.install_id_path = Some(p.into());
        self
    }
    /// Defaults to `<install_id_path>.queue.jsonl` if unset.
    pub fn queue_path(mut self, p: impl Into<PathBuf>) -> Self {
        self.queue_path = Some(p.into());
        self
    }
    /// The app's own opt-out env var name, e.g. `"TOME_TELEMETRY"`.
    pub fn app_env_var(mut self, v: impl Into<String>) -> Self {
        self.app_env_var = Some(v.into());
        self
    }
    /// App config flag (false = user disabled telemetry in config).
    pub fn config_enabled(mut self, v: bool) -> Self {
        self.config_enabled = v;
        self
    }
    /// Runtime toggle (false = disabled for this run).
    pub fn runtime_enabled(mut self, v: bool) -> Self {
        self.runtime_enabled = v;
        self
    }
    pub fn grace(mut self, g: Duration) -> Self {
        self.grace = Some(g);
        self
    }
    /// Args to re-invoke the binary's hidden flush subcommand (detached flush).
    pub fn flush_args(mut self, args: Vec<String>) -> Self {
        self.flush_args = args;
        self
    }
    /// App-supplied acceleration capability (`metal`/`cuda`/`rocm`/`cpu`).
    pub fn accel(mut self, v: impl Into<String>) -> Self {
        self.accel = Some(v.into());
        self
    }

    /// Resolve consent and build the handle. A disabled handle is returned when
    /// consent resolves to off. Returns `Err` only on a genuinely broken setup
    /// (missing required field); telemetry problems are otherwise swallowed.
    pub fn build(self) -> Result<Telemetry, BuildError> {
        let app = self.app.ok_or(BuildError::Missing("app"))?;
        let app_version = self.app_version.ok_or(BuildError::Missing("app_version"))?;
        let endpoint = self.endpoint.ok_or(BuildError::Missing("endpoint"))?;
        let install_id_path = self.install_id_path.ok_or(BuildError::Missing("install_id_path"))?;

        let global = std::env::var(GLOBAL_DISABLE_VAR).ok();
        let app_var = self.app_env_var.as_ref().and_then(|n| std::env::var(n).ok());
        let ci = std::env::var("CI").ok();
        let inputs = ConsentInputs {
            global_disable: global.as_deref(),
            app_var: app_var.as_deref(),
            config_enabled: self.config_enabled,
            runtime_enabled: self.runtime_enabled,
            is_ci: consent::is_ci(ci.as_deref()),
        };
        if !consent::resolve(&inputs) {
            return Ok(Telemetry(None));
        }

        let install_id = identity::load_or_create(&install_id_path)
            .map_err(|e| BuildError::Io(e.to_string()))?;
        let mint_time = identity::mint_time(&install_id_path);
        let queue_path = self
            .queue_path
            .unwrap_or_else(|| install_id_path.with_extension("queue.jsonl"));

        let cfg = SenderConfig {
            endpoint,
            app,
            app_version,
            install_id,
            session_id: Uuid::new_v4(),
            os: env::os_type(),
            arch: env::host_arch(),
            queue_path,
        };
        Ok(Telemetry(Some(Inner {
            cfg,
            grace: self.grace.unwrap_or(DEFAULT_GRACE),
            mint_time,
            env: env::detect(self.accel),
            flush_args: self.flush_args,
            install_id_path,
        })))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("required telemetry config field missing: {0}")]
    Missing(&'static str),
    #[error("telemetry identity io error: {0}")]
    Io(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{CommandInvoked, Outcome, Surface};
    use gauge_events::sender::queue::read_lines;

    fn builder(tmp: &std::path::Path) -> Builder {
        Telemetry::builder()
            .app("tome")
            .app_version("0.7.0")
            .endpoint("http://127.0.0.1:1")
            .install_id_path(tmp.join("id"))
            .config_enabled(true)
            .runtime_enabled(true)
    }

    #[test]
    fn emit_appends_namespaced_event() {
        let tmp = tempfile::tempdir().unwrap();
        let t = builder(tmp.path()).build().unwrap();
        assert!(t.is_enabled());
        t.emit(&CommandInvoked {
            command: "search".into(),
            duration_ms: 10,
            outcome: Outcome::Ok,
            surface: Surface::Cli,
        });
        let lines = read_lines(&tmp.path().join("id.queue.jsonl")).unwrap();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("\"tome.command_invoked\""), "{}", lines[0]);
    }

    #[test]
    fn disabled_handle_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let t = builder(tmp.path()).runtime_enabled(false).build().unwrap();
        assert!(!t.is_enabled());
        t.emit(&CommandInvoked {
            command: "x".into(),
            duration_ms: 0,
            outcome: Outcome::Ok,
            surface: Surface::Cli,
        });
        assert!(!tmp.path().join("id").exists(), "disabled never even mints an install id");
    }

    #[tokio::test]
    async fn flush_blocking_drains_queue_to_server() {
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
            .app("tome")
            .app_version("0.7.0")
            .endpoint(server.uri()) // http://127.0.0.1:PORT — allowed by transport
            .install_id_path(tmp.path().join("id"))
            .config_enabled(true)
            .runtime_enabled(true)
            .grace(std::time::Duration::ZERO) // skip grace so it flushes now
            .build()
            .unwrap();

        t.emit(&CommandInvoked {
            command: "search".into(),
            duration_ms: 5,
            outcome: Outcome::Ok,
            surface: Surface::Cli,
        });
        let queue = tmp.path().join("id.queue.jsonl");
        assert_eq!(read_lines(&queue).unwrap().len(), 1);

        tokio::task::spawn_blocking(move || {
            t.flush_blocking(std::time::Duration::from_secs(5));
        })
        .await
        .unwrap();

        assert!(read_lines(&queue).unwrap().is_empty(), "queue drained after 2xx");
    }
}
