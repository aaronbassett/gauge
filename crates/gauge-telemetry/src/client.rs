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

/// A sane default for the [`Telemetry::flush_blocking`] timeout.
pub const DEFAULT_FLUSH_TIMEOUT: Duration = Duration::from_secs(3);

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
    /// Start configuring a [`Telemetry`] handle.
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
        let name = event.name();
        if !crate::event::is_valid_event_name(&name) {
            debug_assert!(false, "telemetry event name `{name}` is invalid");
            return;
        }
        let full = format!("{}.{}", inner.cfg.app, name);
        let _ = enqueue(&inner.cfg, &full, attrs); // best-effort, non-fatal
    }

    /// Best-effort synchronous flush, capped by `timeout` of wall-clock. Runs
    /// the blocking drain on a worker thread so it is safe to call from async
    /// contexts via `spawn_blocking`. No-op while inside the first-run grace.
    ///
    /// An alternative to (not in addition to) the background [`Flusher`](crate::Flusher).
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

    /// The entrypoint an app routes its hidden flush subcommand to: drain once,
    /// then return (the app exits). Runs in the foreground (this *is* the
    /// detached child process).
    ///
    /// Load-bearing invariant: the app's flush subcommand MUST dispatch to
    /// `run_flush()` and exit before any normal startup path runs — otherwise a
    /// detached flush child can re-enter the app's main flow.
    pub fn run_flush(&self) {
        if let Some(inner) = &self.0 {
            let _ = gauge_events::sender::drain(&inner.cfg);
        }
    }

    /// Spawn a detached child that runs the hidden flush subcommand, then return
    /// immediately. The child survives this process's exit. No-op if disabled,
    /// inside grace, or if `flush_args`/`current_exe` are unavailable.
    ///
    /// Load-bearing invariant: the app's flush subcommand MUST dispatch to
    /// `run_flush()` and exit before any normal startup path runs. Intended for
    /// processes that exit promptly after calling this; long-running processes
    /// should use [`Flusher`](crate::Flusher) instead.
    pub fn spawn_detached_flush(&self) {
        let Some(inner) = &self.0 else {
            return;
        };
        // Never let a flush child spawn its own flush child.
        if std::env::var_os("GAUGE_TELEMETRY_FLUSH_CHILD").is_some() {
            return;
        }
        if inner.flush_args.is_empty()
            || consent::within_grace(inner.mint_time, inner.grace, SystemTime::now())
        {
            return;
        }
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        let mut cmd = std::process::Command::new(exe);
        cmd.args(&inner.flush_args)
            .env("GAUGE_TELEMETRY_FLUSH_CHILD", "1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt as _;
            // New session → detached from the controlling terminal, so the
            // parent's exit/SIGHUP does not kill the flusher.
            unsafe {
                cmd.pre_exec(|| {
                    // setsid failure is non-fatal; the child still flushes
                    libc::setsid();
                    Ok(())
                });
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt as _;
            const DETACHED_PROCESS: u32 = 0x0000_0008;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
            cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
        }
        let _ = cmd.spawn(); // drop the child handle; do not wait
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
    ci_override: Option<bool>,
}

impl Builder {
    /// App name, used as the `<app>.` event-name prefix (required).
    pub fn app(mut self, v: impl Into<String>) -> Self {
        self.app = Some(v.into());
        self
    }
    /// App version, reported as a resource attribute (required).
    pub fn app_version(mut self, v: impl Into<String>) -> Self {
        self.app_version = Some(v.into());
        self
    }
    /// OTLP collector base URL; must be https or loopback (required).
    pub fn endpoint(mut self, v: impl Into<String>) -> Self {
        self.endpoint = Some(v.into());
        self
    }
    /// Path to the persisted install-id file (required).
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
    /// First-run grace period before the first flush (defaults to [`DEFAULT_GRACE`]).
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
    /// Override CI auto-detection. By default the builder treats the process as
    /// CI when the `CI` env var is set to a truthy value (which disables
    /// telemetry). Pass `false` to force non-CI (used by tests that need an
    /// enabled handle while themselves running under CI); `true` to force CI.
    pub fn ci(mut self, is_ci: bool) -> Self {
        self.ci_override = Some(is_ci);
        self
    }

    /// Resolve consent and build the handle. A disabled handle is returned when
    /// consent resolves to off. Returns `Err` only on a genuinely broken setup
    /// (missing required field); telemetry problems are otherwise swallowed.
    pub fn build(self) -> Result<Telemetry, BuildError> {
        let app = self.app.ok_or(BuildError::Missing("app"))?;
        let app_version = self.app_version.ok_or(BuildError::Missing("app_version"))?;
        let endpoint = self.endpoint.ok_or(BuildError::Missing("endpoint"))?;
        let install_id_path = self
            .install_id_path
            .ok_or(BuildError::Missing("install_id_path"))?;

        let global = std::env::var(GLOBAL_DISABLE_VAR).ok();
        let app_var = self
            .app_env_var
            .as_ref()
            .and_then(|n| std::env::var(n).ok());
        let is_ci = self
            .ci_override
            .unwrap_or_else(|| consent::is_ci(std::env::var("CI").ok().as_deref()));
        let inputs = ConsentInputs {
            global_disable: global.as_deref(),
            app_var: app_var.as_deref(),
            config_enabled: self.config_enabled,
            runtime_enabled: self.runtime_enabled,
            is_ci,
        };
        if !consent::resolve(&inputs) {
            return Ok(Telemetry(None));
        }

        // Fail fast on a misconfigured endpoint: a non-https/non-loopback URL
        // would pass build() but silently fail every drain, filling the queue
        // until events are dropped. Placed after the consent early-return (so a
        // disabled handle stays a pure no-op) and before identity minting (so a
        // misconfigured enabled build never touches the filesystem).
        if !gauge_events::sender::transport::endpoint_allowed(&endpoint) {
            return Err(BuildError::InsecureEndpoint(endpoint));
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
    #[error("telemetry endpoint must be https or loopback: {0}")]
    InsecureEndpoint(String),
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
            .ci(false)
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
        assert!(
            lines[0].contains("\"tome.command_invoked\""),
            "{}",
            lines[0]
        );
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
        assert!(
            !tmp.path().join("id").exists(),
            "disabled never even mints an install id"
        );
    }

    #[test]
    fn ci_detection_disables_via_override() {
        let tmp = tempfile::tempdir().unwrap();
        let t = Telemetry::builder()
            .app("tome")
            .app_version("0.7.0")
            .endpoint("https://example.invalid")
            .install_id_path(tmp.path().join("id"))
            .config_enabled(true)
            .runtime_enabled(true)
            .ci(true)
            .build()
            .unwrap();
        assert!(!t.is_enabled());
        assert!(!tmp.path().join("id").exists()); // disabled => no FS work
    }

    #[test]
    fn insecure_endpoint_is_rejected_at_build() {
        let tmp = tempfile::tempdir().unwrap();
        let result = Telemetry::builder()
            .app("tome")
            .app_version("0.7.0")
            .endpoint("http://telemetry.internal") // plain http, non-loopback
            .install_id_path(tmp.path().join("id"))
            .config_enabled(true)
            .runtime_enabled(true)
            .ci(false)
            .build();
        // `Telemetry` is not `Debug`, so match rather than `unwrap_err`.
        match result {
            Err(BuildError::InsecureEndpoint(_)) => {}
            Err(other) => panic!("expected InsecureEndpoint, got {other:?}"),
            Ok(_) => panic!("expected InsecureEndpoint, got Ok"),
        }
        // The enabled-but-misconfigured build must not have minted an id.
        assert!(!tmp.path().join("id").exists());
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
            .ci(false)
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

        assert!(
            read_lines(&queue).unwrap().is_empty(),
            "queue drained after 2xx"
        );
    }

    #[tokio::test]
    async fn run_flush_drains_like_blocking() {
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
            .endpoint(server.uri())
            .install_id_path(tmp.path().join("id"))
            .config_enabled(true)
            .runtime_enabled(true)
            .ci(false)
            .grace(std::time::Duration::ZERO)
            .build()
            .unwrap();
        t.emit(&CommandInvoked {
            command: "x".into(),
            duration_ms: 1,
            outcome: Outcome::Ok,
            surface: Surface::Cli,
        });
        let queue = tmp.path().join("id.queue.jsonl");

        tokio::task::spawn_blocking(move || t.run_flush())
            .await
            .unwrap();
        assert!(read_lines(&queue).unwrap().is_empty());
    }
}
