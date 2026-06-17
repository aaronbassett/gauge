# gauge-telemetry Client Kernel Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `gauge-telemetry`, a publishable "fat kernel" crate that internal apps use to send privacy-first telemetry to the Gauge backend — owning events, identity, consent, environment capture, and the flush lifecycle on top of `gauge-events`' existing disk-queue sender.

**Architecture:** A new library crate in the Gauge workspace that depends on `gauge-events` (feature `sender`) and reuses its `SenderConfig`/`enqueue`/`drain` rather than reinventing the queue or transport. The kernel adds: a serde-typed `Event` trait + common event types, install/session identity, a layered opt-out consent resolver, coarse environment capture, and three flush triggers (detached at-exit, background, and a blocking fallback). All quantities are sent as raw integers; bucketing is deferred to the server (issue #22). Scope is the crate only — porting tome/midnight-manual is out of scope.

**Tech Stack:** Rust 2024 (edition 2024, resolver 3, rust 1.93), `gauge-events`, `serde`/`serde_json`, `uuid` (v4), `time`, `thiserror`, `sysinfo` (RAM + OS version), `libc` (unix, for `setsid`). Dev: `tempfile`, `wiremock`, `tokio`, `insta`.

Design spec: `docs/superpowers/specs/2026-06-17-gauge-telemetry-kernel-design.md`.

---

## File Structure

```
crates/gauge-telemetry/
  Cargo.toml
  SPEC.md                         # wire contract, byte-pinned by a test
  PORTING.md                      # deliverable: tome + mn migration plan
  src/
    lib.rs                        # crate docs + re-exports
    event.rs                      # Event trait + to_attributes() scalar validation
    env.rs                        # EnvAttributes + detection + os/arch remap
    common.rs                     # common event types + shared enums (Outcome, Surface)
    identity.rs                   # install UUID (persisted) + session UUID
    consent.rs                    # pure opt-out resolver + grace-period check
    client.rs                     # Telemetry handle + builder + emit + flush_blocking
    flush.rs                      # background Flusher + detached at-exit flush
    canary.rs                     # reusable canary harness for apps
  tests/
    conformance.rs                # emitted events pass gauge-events::validate_batch
    canary_suite.rs               # kernel's own common events carry no forbidden strings
    spec_pin.rs                   # byte-pinned OTLP worked example vs SPEC.md
```

One responsibility per file. `env.rs` owns `EnvAttributes` (consumed by `common.rs`). `consent.rs` is pure (no env reads) so it is unit-testable; `client.rs` reads the real process environment and calls into it.

---

## Task 0: Scaffold the crate and wire it into the workspace

**Files:**
- Create: `crates/gauge-telemetry/Cargo.toml`
- Create: `crates/gauge-telemetry/src/lib.rs`
- Modify: `Cargo.toml` (workspace `members`)

- [ ] **Step 1: Create the crate manifest**

Create `crates/gauge-telemetry/Cargo.toml`:

```toml
[package]
name = "gauge-telemetry"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
repository.workspace = true
homepage.workspace = true
authors.workspace = true
description = "Privacy-first telemetry client kernel for Gauge: typed events, identity, consent, and a crash-safe sender."
keywords = ["telemetry", "otlp", "analytics", "privacy"]
categories = ["development-tools"]
readme = "SPEC.md"

[dependencies]
gauge-events = { workspace = true, features = ["sender"] }
serde = { workspace = true }
serde_json = { workspace = true }
uuid = { workspace = true }
time = { workspace = true }
thiserror = { workspace = true }
sysinfo = "0.33"

[target.'cfg(unix)'.dependencies]
libc = "0.2"

[dev-dependencies]
tempfile = { workspace = true }
wiremock = { workspace = true }
tokio = { workspace = true }
insta = { workspace = true }
```

- [ ] **Step 2: Create a minimal lib.rs**

Create `crates/gauge-telemetry/src/lib.rs`:

```rust
//! `gauge-telemetry` — privacy-first telemetry client kernel for Gauge.
//!
//! Apps build a [`Telemetry`] handle once at startup, then `emit` typed events.
//! The hot path only appends one line to a disk queue; delivery happens out of
//! band. See `SPEC.md` for the wire contract.

pub mod canary;
pub mod client;
pub mod common;
pub mod consent;
pub mod env;
pub mod event;
pub mod flush;

/// Crate version, stamped at build time.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
```

> Note: this will not compile until the modules exist. Add the `pub mod` lines incrementally as you create each module, OR create empty module files now (`echo` an empty file) so the crate compiles. Recommended: create empty stub files for each module in this step.

- [ ] **Step 3: Create empty module stubs so the crate compiles**

Create these empty files: `src/event.rs`, `src/env.rs`, `src/common.rs`, `src/identity.rs`, `src/consent.rs`, `src/client.rs`, `src/flush.rs`, `src/canary.rs`. Add `mod identity;` is intentionally **not** public — keep it `pub(crate)`; add to lib.rs:

```rust
pub(crate) mod identity;
```

- [ ] **Step 4: Add the crate to the workspace members**

In the root `Cargo.toml`, add `"crates/gauge-telemetry"` to `[workspace] members`:

```toml
members = [
    "crates/gauge-auth",
    "crates/gauge-events",
    "crates/gauge-query",
    "crates/gauge-server",
    "crates/gauge",
    "crates/gauge-telemetry",
]
```

- [ ] **Step 5: Verify it builds**

Run: `cargo build -p gauge-telemetry`
Expected: compiles (warnings about empty/unused modules are fine).

- [ ] **Step 6: Commit**

```bash
git add crates/gauge-telemetry Cargo.toml
git commit -m "feat(telemetry): scaffold gauge-telemetry crate"
```

---

## Task 1: Event trait and scalar-attribute conversion

**Files:**
- Modify: `crates/gauge-telemetry/src/event.rs`

- [ ] **Step 1: Write the failing test**

In `src/event.rs`:

```rust
//! The `Event` trait and the serialize → flat scalar-attribute-map conversion
//! that every emitted event passes through. Non-scalar fields are rejected;
//! this is the structural half of the privacy contract (canary tests are the
//! other half — see `canary`).

use std::borrow::Cow;

use serde::Serialize;
use serde_json::{Map, Value};

use gauge_events::profile::{MAX_ATTRIBUTES_PER_RECORD, MAX_ATTR_STRING_BYTES};

/// Anything an app emits. The bare `name()` is namespaced with `<app>.` by the
/// client; the value must serialize (via serde) to a flat JSON object whose
/// values are all scalars (string / bool / number).
pub trait Event: Serialize {
    /// Bare event name, no app prefix, e.g. `"search"` → `"tome.search"`.
    fn name(&self) -> Cow<'_, str>;
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum EmitError {
    #[error("event must serialize to a JSON object")]
    NotAnObject,
    #[error("attribute `{0}` is not a scalar (string/bool/number)")]
    NonScalar(String),
    #[error("attribute `{0}` string value exceeds {MAX_ATTR_STRING_BYTES} bytes")]
    StringTooLong(String),
    #[error("event has {0} attributes, exceeding {MAX_ATTRIBUTES_PER_RECORD}")]
    TooManyAttributes(usize),
}

/// Convert an event into the flat scalar attribute map the sender expects.
/// `null` values (e.g. `None` fields) are omitted; nested values are rejected.
pub fn to_attributes<E: Event + ?Sized>(event: &E) -> Result<Map<String, Value>, EmitError> {
    let value = serde_json::to_value(event).map_err(|_| EmitError::NotAnObject)?;
    let Value::Object(obj) = value else {
        return Err(EmitError::NotAnObject);
    };
    let mut out = Map::new();
    for (k, v) in obj {
        match v {
            Value::Null => continue,
            Value::String(ref s) => {
                if s.len() > MAX_ATTR_STRING_BYTES {
                    return Err(EmitError::StringTooLong(k));
                }
                out.insert(k, v);
            }
            Value::Bool(_) | Value::Number(_) => {
                out.insert(k, v);
            }
            Value::Array(_) | Value::Object(_) => return Err(EmitError::NonScalar(k)),
        }
    }
    if out.len() > MAX_ATTRIBUTES_PER_RECORD {
        return Err(EmitError::TooManyAttributes(out.len()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize)]
    struct Sample {
        s: String,
        n: u32,
        b: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        maybe: Option<String>,
    }
    impl Event for Sample {
        fn name(&self) -> Cow<'_, str> {
            "sample".into()
        }
    }

    #[test]
    fn scalars_pass_and_none_is_omitted() {
        let e = Sample { s: "x".into(), n: 7, b: true, maybe: None };
        let attrs = to_attributes(&e).unwrap();
        assert_eq!(attrs.len(), 3);
        assert_eq!(attrs["s"], Value::String("x".into()));
        assert_eq!(attrs["n"], Value::Number(7u32.into()));
        assert_eq!(attrs["b"], Value::Bool(true));
        assert!(!attrs.contains_key("maybe"));
    }

    #[derive(Serialize)]
    struct Nested {
        inner: Vec<u8>,
    }
    impl Event for Nested {
        fn name(&self) -> Cow<'_, str> {
            "nested".into()
        }
    }

    #[test]
    fn nested_value_is_rejected() {
        let e = Nested { inner: vec![1, 2] };
        assert_eq!(to_attributes(&e), Err(EmitError::NonScalar("inner".into())));
    }

    #[derive(Serialize)]
    struct LongString {
        big: String,
    }
    impl Event for LongString {
        fn name(&self) -> Cow<'_, str> {
            "long".into()
        }
    }

    #[test]
    fn overlong_string_is_rejected() {
        let e = LongString { big: "x".repeat(MAX_ATTR_STRING_BYTES + 1) };
        assert_eq!(to_attributes(&e), Err(EmitError::StringTooLong("big".into())));
    }
}
```

- [ ] **Step 2: Run the tests to verify they pass**

The implementation is included above (test-first here means writing the module with its tests). Run:
`cargo test -p gauge-telemetry event::`
Expected: 3 tests PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/gauge-telemetry/src/event.rs
git commit -m "feat(telemetry): Event trait + scalar attribute conversion"
```

---

## Task 2: Environment capture and os/arch remap

**Files:**
- Modify: `crates/gauge-telemetry/src/env.rs`

- [ ] **Step 1: Write the module with tests**

In `src/env.rs`:

```rust
//! Coarse environment capture for the `install`/`heartbeat` events, plus the
//! `os.type`/`host.arch` remaps the Gauge profile requires. Everything is
//! best-effort: a field that can't be detected is omitted (`None`).

use serde::Serialize;

/// Coarse, low-cardinality environment attributes. Sent only on low-frequency
/// lifecycle events. Quantities are raw integers (bucketed at read time).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct EnvAttributes {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_cores: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ram_gb: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub libc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
}

/// `os.type` resource attribute, remapped to the Gauge profile vocabulary.
pub fn os_type() -> String {
    match std::env::consts::OS {
        "macos" => "darwin",
        other => other, // "linux", "windows"
    }
    .to_string()
}

/// `host.arch` resource attribute, remapped to the Gauge profile vocabulary.
pub fn host_arch() -> String {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    }
    .to_string()
}

/// libc is a compile-time property on Linux; `None` off Linux.
fn libc() -> Option<String> {
    if cfg!(target_os = "linux") {
        Some(if cfg!(target_env = "musl") { "musl" } else { "glibc" }.to_string())
    } else {
        None
    }
}

/// Language subtag only, e.g. `en_US.UTF-8` → `en`. Pure for testability.
pub fn language_from(lang: Option<&str>) -> Option<String> {
    let raw = lang?.trim();
    if raw.is_empty() || raw == "C" || raw == "POSIX" {
        return None;
    }
    let subtag = raw.split(['_', '.', '-']).next().unwrap_or(raw);
    (!subtag.is_empty()).then(|| subtag.to_ascii_lowercase())
}

/// Map a `$SHELL` path to a closed enum string. Pure for testability.
pub fn shell_from(shell_path: Option<&str>) -> Option<String> {
    let base = std::path::Path::new(shell_path?.trim())
        .file_name()?
        .to_str()?
        .to_ascii_lowercase();
    Some(
        match base.as_str() {
            "bash" => "bash",
            "zsh" => "zsh",
            "fish" => "fish",
            "pwsh" | "powershell" => "pwsh",
            "cmd" | "cmd.exe" => "cmd",
            _ => "other",
        }
        .to_string(),
    )
}

/// Detect everything. `accel` is supplied by the app (it knows its inference
/// backend better than any detector); pass `None` to omit it.
pub fn detect(accel: Option<String>) -> EnvAttributes {
    EnvAttributes {
        os_version: os_version(),
        cpu_cores: std::thread::available_parallelism().ok().map(|n| n.get() as u32),
        ram_gb: ram_gb(),
        accel,
        libc: libc(),
        language: language_from(std::env::var("LANG").ok().as_deref()),
        shell: shell_from(std::env::var("SHELL").ok().as_deref()),
    }
}

/// `<id>:<major>` e.g. `darwin:14`, `ubuntu:22`, `windows:11`. Best-effort.
fn os_version() -> Option<String> {
    // sysinfo associated fns; verify exact names against the resolved 0.33 API.
    let id = sysinfo::System::distribution_id();
    let id = if id == "macos" { "darwin".to_string() } else { id };
    let ver = sysinfo::System::os_version()?; // e.g. "14.5", "22.04", "11"
    let major = ver.split(['.', ' ']).next().filter(|s| !s.is_empty())?;
    Some(format!("{id}:{major}"))
}

/// Total physical RAM rounded to whole GB. Best-effort.
fn ram_gb() -> Option<u32> {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    let bytes = sys.total_memory(); // sysinfo >= 0.30 returns BYTES
    if bytes == 0 {
        return None;
    }
    Some((bytes as f64 / 1_073_741_824.0).round() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_and_arch_are_profile_vocab() {
        // os_type/host_arch must only ever produce profile-legal values.
        assert!(["darwin", "linux", "windows"].contains(&os_type().as_str()));
        assert!(["amd64", "arm64"].contains(&host_arch().as_str()) || !host_arch().is_empty());
    }

    #[test]
    fn language_subtag_only() {
        assert_eq!(language_from(Some("en_US.UTF-8")).as_deref(), Some("en"));
        assert_eq!(language_from(Some("de_DE")).as_deref(), Some("de"));
        assert_eq!(language_from(Some("C")), None);
        assert_eq!(language_from(None), None);
    }

    #[test]
    fn shell_maps_to_enum() {
        assert_eq!(shell_from(Some("/bin/zsh")).as_deref(), Some("zsh"));
        assert_eq!(shell_from(Some("/usr/bin/fish")).as_deref(), Some("fish"));
        assert_eq!(shell_from(Some("/opt/weird/tcsh")).as_deref(), Some("other"));
        assert_eq!(shell_from(None), None);
    }

    #[test]
    fn detect_does_not_panic_and_sees_cpus() {
        let env = detect(Some("cpu".into()));
        assert!(env.cpu_cores.unwrap_or(0) >= 1);
        assert_eq!(env.accel.as_deref(), Some("cpu"));
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p gauge-telemetry env::`
Expected: 4 tests PASS.

> If `os_version`/`ram_gb` fail to compile, check the resolved `sysinfo` version's API: in 0.30+, `total_memory()` returns bytes and `System::os_version()`/`System::distribution_id()` are associated functions. Adjust the two helper bodies only; the tests don't assert their exact values.

- [ ] **Step 3: Commit**

```bash
git add crates/gauge-telemetry/src/env.rs
git commit -m "feat(telemetry): environment capture + os/arch remap"
```

---

## Task 3: Common events and shared enums

**Files:**
- Modify: `crates/gauge-telemetry/src/common.rs`

- [ ] **Step 1: Write the module with tests**

In `src/common.rs`:

```rust
//! The shared "common event" types — the ergonomic shortcuts. App-specific
//! events stay as the app's own `Event` types. This is a proposed starting set
//! (per the design spec) and is extended additively during porting.

use std::borrow::Cow;

use serde::Serialize;

use crate::env::EnvAttributes;
use crate::event::Event;

/// Coarse outcome — union of the two apps' outcome enums.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Ok,
    Failed,
    InvalidInput,
}

/// Where the activity happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
    Cli,
    Mcp,
}

/// First run on this install. Carries the environment snapshot.
#[derive(Debug, Clone, Serialize)]
pub struct Install {
    pub install_method: String,
    #[serde(flatten)]
    pub env: EnvAttributes,
}
impl Event for Install {
    fn name(&self) -> Cow<'_, str> {
        "install".into()
    }
}

/// Periodic liveness event (cadence chosen by the app). Carries the environment.
#[derive(Debug, Clone, Serialize)]
pub struct Heartbeat {
    #[serde(flatten)]
    pub env: EnvAttributes,
}
impl Event for Heartbeat {
    fn name(&self) -> Cow<'_, str> {
        "heartbeat".into()
    }
}

/// One top-level command/subcommand completed.
#[derive(Debug, Clone, Serialize)]
pub struct CommandInvoked {
    pub command: String,
    pub duration_ms: u32,
    pub outcome: Outcome,
    pub surface: Surface,
}
impl Event for CommandInvoked {
    fn name(&self) -> Cow<'_, str> {
        "command_invoked".into()
    }
}

/// One tool/operation invocation completed.
#[derive(Debug, Clone, Serialize)]
pub struct ToolCall {
    pub tool: String,
    pub latency_ms: u32,
    pub result_count: u32,
    pub outcome: Outcome,
}
impl Event for ToolCall {
    fn name(&self) -> Cow<'_, str> {
        "tool_call".into()
    }
}

/// A failure, classified by a closed `error_class` (never a raw message).
#[derive(Debug, Clone, Serialize)]
pub struct ErrorEvent {
    pub error_class: String,
    pub surface: Surface,
}
impl Event for ErrorEvent {
    fn name(&self) -> Cow<'_, str> {
        "error".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::to_attributes;
    use serde_json::Value;

    #[test]
    fn command_invoked_serializes_to_expected_scalars() {
        let e = CommandInvoked {
            command: "search".into(),
            duration_ms: 142,
            outcome: Outcome::Ok,
            surface: Surface::Cli,
        };
        assert_eq!(e.name(), "command_invoked");
        let a = to_attributes(&e).unwrap();
        assert_eq!(a["command"], Value::String("search".into()));
        assert_eq!(a["duration_ms"], Value::Number(142u32.into()));
        assert_eq!(a["outcome"], Value::String("ok".into()));
        assert_eq!(a["surface"], Value::String("cli".into()));
    }

    #[test]
    fn heartbeat_flattens_env_and_omits_missing() {
        let e = Heartbeat {
            env: EnvAttributes { cpu_cores: Some(8), accel: Some("metal".into()), ..Default::default() },
        };
        let a = to_attributes(&e).unwrap();
        assert_eq!(a["cpu_cores"], Value::Number(8u32.into()));
        assert_eq!(a["accel"], Value::String("metal".into()));
        assert!(!a.contains_key("ram_gb")); // None omitted, no nested object
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p gauge-telemetry common::`
Expected: 2 tests PASS. (The `heartbeat_flattens_env` test confirms `#[serde(flatten)]` produces a flat object that `to_attributes` accepts.)

- [ ] **Step 3: Commit**

```bash
git add crates/gauge-telemetry/src/common.rs
git commit -m "feat(telemetry): common event types + shared enums"
```

---

## Task 4: Identity (install + session UUIDs)

**Files:**
- Modify: `crates/gauge-telemetry/src/identity.rs`

- [ ] **Step 1: Write the module with tests**

In `src/identity.rs`:

```rust
//! Install identity: a random v4 UUID persisted `0600`, created race-safely and
//! reused thereafter. The session UUID is minted per process and never stored.

use std::io;
use std::path::Path;
use std::time::SystemTime;

use uuid::Uuid;

/// Load the install UUID, creating it on first run. Race-safe: a concurrent
/// process either creates it or loses the race and reads the winner's value.
pub fn load_or_create(path: &Path) -> io::Result<Uuid> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        opts.mode(0o600);
    }
    match opts.open(path) {
        Ok(_) => {
            let id = Uuid::new_v4();
            std::fs::write(path, id.to_string())?;
            Ok(id)
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => read(path),
        Err(e) => Err(e),
    }
}

fn read(path: &Path) -> io::Result<Uuid> {
    let s = std::fs::read_to_string(path)?;
    Uuid::parse_str(s.trim()).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Regenerate the install UUID (severing future continuity). Overwrites in place.
pub fn reset(path: &Path) -> io::Result<Uuid> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let id = Uuid::new_v4();
    std::fs::write(path, id.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(id)
}

/// The install file's mtime = mint time, used for the first-run grace period.
pub fn mint_time(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_then_reuse_is_stable() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("sub/id");
        let a = load_or_create(&p).unwrap();
        let b = load_or_create(&p).unwrap();
        assert_eq!(a, b, "second call reuses the persisted UUID");
        assert!(mint_time(&p).is_some());
    }

    #[test]
    fn reset_changes_the_uuid() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("id");
        let a = load_or_create(&p).unwrap();
        let b = reset(&p).unwrap();
        assert_ne!(a, b);
        assert_eq!(b, load_or_create(&p).unwrap(), "reset value persists");
    }

    #[cfg(unix)]
    #[test]
    fn file_is_0600() {
        use std::os::unix::fs::PermissionsExt as _;
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("id");
        load_or_create(&p).unwrap();
        assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p gauge-telemetry identity::`
Expected: 3 tests PASS (2 on non-unix).

- [ ] **Step 3: Commit**

```bash
git add crates/gauge-telemetry/src/identity.rs
git commit -m "feat(telemetry): install + session identity"
```

---

## Task 5: Consent resolver (pure)

**Files:**
- Modify: `crates/gauge-telemetry/src/consent.rs`

- [ ] **Step 1: Write the module with tests**

In `src/consent.rs`. The resolver is **pure** — it takes already-read inputs so tests never mutate process env:

```rust
//! Opt-out consent resolution. Pure: callers (the client builder) read the
//! process environment and pass values in. Disabled wins.

use std::time::{Duration, SystemTime};

/// The global kill-switch env var. `=1` forces telemetry off for ANY app.
pub const GLOBAL_DISABLE_VAR: &str = "GAUGE_TELEMETRY_DISABLE";

/// Inputs to the resolver, all already read from the environment/config.
#[derive(Debug, Clone, Default)]
pub struct ConsentInputs<'a> {
    /// Value of `GAUGE_TELEMETRY_DISABLE`, if set.
    pub global_disable: Option<&'a str>,
    /// Value of the app's own opt-out env var (e.g. `TOME_TELEMETRY`), if set.
    pub app_var: Option<&'a str>,
    /// The app config flag (true = user has not disabled in config).
    pub config_enabled: bool,
    /// The runtime toggle (true = not toggled off this run).
    pub runtime_enabled: bool,
    /// Whether a CI environment was detected.
    pub is_ci: bool,
}

/// Resolve whether telemetry is enabled. Disabled wins; the global var can only
/// ever disable (`=1`), never force-enable.
pub fn resolve(i: &ConsentInputs<'_>) -> bool {
    if i.global_disable == Some("1") {
        return false;
    }
    if !i.runtime_enabled || !i.config_enabled {
        return false;
    }
    if is_falsey(i.app_var) {
        return false;
    }
    if i.is_ci {
        return false;
    }
    true
}

/// `Some("0"|"false"|"off"|"no")` (case-insensitive) is a disable signal.
/// Unset or any other value is NOT a disable signal (and never force-enables).
fn is_falsey(v: Option<&str>) -> bool {
    matches!(
        v.map(|s| s.trim().to_ascii_lowercase()).as_deref(),
        Some("0" | "false" | "off" | "no")
    )
}

/// Detect CI from the conventional `CI` env var value.
pub fn is_ci(ci_var: Option<&str>) -> bool {
    !is_falsey_or_unset(ci_var)
}

fn is_falsey_or_unset(v: Option<&str>) -> bool {
    match v.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        None | Some("" | "0" | "false" | "off" | "no") => true,
        _ => false,
    }
}

/// True while still inside the first-run grace window (flush should wait).
pub fn within_grace(mint: Option<SystemTime>, grace: Duration, now: SystemTime) -> bool {
    let Some(mint) = mint else {
        return false; // unknown mint time → don't hold delivery
    };
    match now.duration_since(mint) {
        Ok(elapsed) => elapsed < grace,
        Err(_) => true, // clock skew (mint in the future) → still within grace
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> ConsentInputs<'static> {
        ConsentInputs { config_enabled: true, runtime_enabled: true, ..Default::default() }
    }

    #[test]
    fn enabled_by_default() {
        assert!(resolve(&base()));
    }

    #[test]
    fn global_kill_switch_overrides_app_opt_in() {
        let i = ConsentInputs { global_disable: Some("1"), app_var: Some("1"), ..base() };
        assert!(!resolve(&i));
    }

    #[test]
    fn global_zero_does_not_force_enable() {
        // global=0 + app says off → still disabled (defer to app)
        let i = ConsentInputs { global_disable: Some("0"), app_var: Some("0"), ..base() };
        assert!(!resolve(&i));
    }

    #[test]
    fn app_falsey_disables() {
        for v in ["0", "false", "OFF", "No"] {
            let i = ConsentInputs { app_var: Some(v), ..base() };
            assert!(!resolve(&i), "{v} should disable");
        }
    }

    #[test]
    fn ci_disables() {
        assert!(!resolve(&ConsentInputs { is_ci: true, ..base() }));
        assert!(is_ci(Some("true")));
        assert!(!is_ci(Some("0")));
        assert!(!is_ci(None));
    }

    #[test]
    fn grace_window() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1000);
        let recent = now - Duration::from_secs(60);
        let old = now - Duration::from_secs(3600);
        assert!(within_grace(Some(recent), Duration::from_secs(600), now));
        assert!(!within_grace(Some(old), Duration::from_secs(600), now));
        assert!(!within_grace(None, Duration::from_secs(600), now));
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p gauge-telemetry consent::`
Expected: 6 tests PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/gauge-telemetry/src/consent.rs
git commit -m "feat(telemetry): pure opt-out consent resolver + grace check"
```

---

## Task 6: Telemetry handle, builder, and `emit`

**Files:**
- Modify: `crates/gauge-telemetry/src/client.rs`
- Modify: `crates/gauge-telemetry/src/lib.rs` (re-export `Telemetry`)

- [ ] **Step 1: Write the module with tests**

In `src/client.rs`:

```rust
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
}

/// The telemetry handle. `None` inner = disabled (consent resolved to off).
pub struct Telemetry(pub(crate) Option<Inner>);

impl Telemetry {
    pub fn builder() -> Builder {
        Builder::default()
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

    /// Regenerate the install UUID and clear the queue.
    pub fn reset(&self) -> std::io::Result<()> {
        let Some(inner) = &self.0 else {
            return Ok(());
        };
        identity::reset(&install_id_path(&inner.cfg))?;
        let _ = std::fs::remove_file(&inner.cfg.queue_path);
        Ok(())
    }
}

fn install_id_path(_cfg: &SenderConfig) -> PathBuf {
    // The install-id path is captured by the builder; stored separately so reset
    // can find it. See Builder::build, which sets it on Inner via cfg sibling.
    unreachable!("replaced in Step 3")
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
            install_id_path, // <-- store for reset(); see Step 3
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
}
```

- [ ] **Step 2: Fix the `Inner.install_id_path` field and `reset()`**

The draft above references an `install_id_path` field on `Inner` and a stub `install_id_path()` fn. Replace them with a real field. Edit `Inner` to add the field and rewrite `reset()`/delete the stub:

```rust
pub(crate) struct Inner {
    pub cfg: SenderConfig,
    pub grace: Duration,
    pub mint_time: Option<SystemTime>,
    pub env: EnvAttributes,
    pub flush_args: Vec<String>,
    pub install_id_path: PathBuf,
}
```

Rewrite `reset()` and delete `fn install_id_path(...)`:

```rust
    pub fn reset(&self) -> std::io::Result<()> {
        let Some(inner) = &self.0 else {
            return Ok(());
        };
        identity::reset(&inner.install_id_path)?;
        let _ = std::fs::remove_file(&inner.cfg.queue_path);
        Ok(())
    }
```

- [ ] **Step 3: Re-export `Telemetry` from lib.rs**

Add to `src/lib.rs`:

```rust
pub use client::{BuildError, Builder, Telemetry};
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p gauge-telemetry client::`
Expected: 2 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/gauge-telemetry/src/client.rs crates/gauge-telemetry/src/lib.rs
git commit -m "feat(telemetry): Telemetry handle, builder, and emit"
```

---

## Task 7: Blocking flush (delivery) with grace gating

**Files:**
- Modify: `crates/gauge-telemetry/src/client.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/client.rs` (uses `wiremock` to stand in for the server):

```rust
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
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p gauge-telemetry client::tests::flush_blocking_drains_queue_to_server`
Expected: FAIL — `flush_blocking` not found.

- [ ] **Step 3: Implement `flush_blocking`**

Add to `impl Telemetry` in `src/client.rs`:

```rust
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
```

Add the `drain` import is already covered by the fully-qualified `gauge_events::sender::drain` call above.

- [ ] **Step 4: Run it to verify it passes**

Run: `cargo test -p gauge-telemetry client::tests::flush_blocking_drains_queue_to_server`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/gauge-telemetry/src/client.rs
git commit -m "feat(telemetry): blocking flush with grace gating"
```

---

## Task 8: Background flusher

**Files:**
- Modify: `crates/gauge-telemetry/src/flush.rs`
- Modify: `crates/gauge-telemetry/src/lib.rs` (re-export `Flusher`)

- [ ] **Step 1: Write the module with tests**

In `src/flush.rs`:

```rust
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
    /// Drains every `interval`, or sooner if the queue exceeds `threshold_bytes`.
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
    // without waiting the full interval.
    let tick = Duration::from_millis(100).min(interval);
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
```

- [ ] **Step 2: Add the `Telemetry::inner()` accessor used by the flusher**

In `src/client.rs`, add a crate-visible accessor on `impl Telemetry`:

```rust
    pub(crate) fn inner(&self) -> Option<&Inner> {
        self.0.as_ref()
    }
```

- [ ] **Step 3: Re-export `Flusher` from lib.rs**

Add to `src/lib.rs`:

```rust
pub use flush::Flusher;
```

- [ ] **Step 4: Run the test**

Run: `cargo test -p gauge-telemetry flush::`
Expected: PASS (within ~2s).

- [ ] **Step 5: Commit**

```bash
git add crates/gauge-telemetry/src/flush.rs crates/gauge-telemetry/src/client.rs crates/gauge-telemetry/src/lib.rs
git commit -m "feat(telemetry): background flusher (interval + threshold)"
```

---

## Task 9: Detached at-exit flush + `run_flush`

**Files:**
- Modify: `crates/gauge-telemetry/src/flush.rs`
- Modify: `crates/gauge-telemetry/src/client.rs`

- [ ] **Step 1: Write the failing test (pure command-parts builder + run_flush)**

The actual process detach can't be unit-tested portably, so we test the **argument construction** and the **`run_flush` drain path**. Add to `src/flush.rs`:

```rust
/// Build the (program, args) the detached flusher will exec: the current
/// binary re-invoked with the app-registered flush args. Pure for testing.
pub fn detached_command_parts(current_exe: &std::path::Path, flush_args: &[String]) -> (String, Vec<String>) {
    (current_exe.display().to_string(), flush_args.to_vec())
}

#[cfg(test)]
mod detach_tests {
    use super::*;

    #[test]
    fn command_parts_reexec_current_exe_with_flush_args() {
        let exe = std::path::Path::new("/usr/local/bin/tome");
        let args = vec!["telemetry".to_string(), "flush".to_string(), "--quiet".to_string()];
        let (prog, got) = detached_command_parts(exe, &args);
        assert_eq!(prog, "/usr/local/bin/tome");
        assert_eq!(got, args);
    }
}
```

Add to the existing `tests` module in `src/client.rs`:

```rust
    #[tokio::test]
    async fn run_flush_drains_like_blocking() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/v1/logs"))
            .respond_with(ResponseTemplate::new(200)).mount(&server).await;

        let tmp = tempfile::tempdir().unwrap();
        let t = Telemetry::builder().app("tome").app_version("0.7.0")
            .endpoint(server.uri()).install_id_path(tmp.path().join("id"))
            .config_enabled(true).runtime_enabled(true).grace(std::time::Duration::ZERO)
            .build().unwrap();
        t.emit(&CommandInvoked { command: "x".into(), duration_ms: 1, outcome: Outcome::Ok, surface: Surface::Cli });
        let queue = tmp.path().join("id.queue.jsonl");

        tokio::task::spawn_blocking(move || t.run_flush()).await.unwrap();
        assert!(read_lines(&queue).unwrap().is_empty());
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p gauge-telemetry run_flush_drains_like_blocking`
Expected: FAIL — `run_flush` not found.

- [ ] **Step 3: Implement `run_flush` and `spawn_detached_flush`**

Add to `impl Telemetry` in `src/client.rs`:

```rust
    /// The entrypoint an app routes its hidden flush subcommand to: drain once,
    /// then return (the app exits). Runs in the foreground (this *is* the
    /// detached child process).
    pub fn run_flush(&self) {
        if let Some(inner) = &self.0 {
            let _ = gauge_events::sender::drain(&inner.cfg);
        }
    }

    /// Spawn a detached child that runs the hidden flush subcommand, then return
    /// immediately. The child survives this process's exit. No-op if disabled,
    /// inside grace, or if `flush_args`/`current_exe` are unavailable.
    pub fn spawn_detached_flush(&self) {
        let Some(inner) = &self.0 else {
            return;
        };
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
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p gauge-telemetry`
Expected: all PASS, including `detach_tests::command_parts_reexec_current_exe_with_flush_args` and `run_flush_drains_like_blocking`.

> Manual verification (not unit-tested): wire a hidden `flush` subcommand in a real CLI that calls `telemetry.run_flush()`, call `telemetry.spawn_detached_flush()` at exit, and confirm with `ps` that the detached child completes after the parent exits. Verify Windows behaviour here (spec open question).

- [ ] **Step 5: Commit**

```bash
git add crates/gauge-telemetry/src/flush.rs crates/gauge-telemetry/src/client.rs
git commit -m "feat(telemetry): detached at-exit flush + run_flush entrypoint"
```

---

## Task 10: Canary harness + canary suite for common events

**Files:**
- Modify: `crates/gauge-telemetry/src/canary.rs`
- Modify: `crates/gauge-telemetry/src/lib.rs` (re-export)
- Create: `crates/gauge-telemetry/tests/canary_suite.rs`

- [ ] **Step 1: Write the canary harness with a unit test**

In `src/canary.rs`:

```rust
//! A reusable canary harness. Apps build event instances that stuff a forbidden
//! string into every string-typed field, then assert none of it reaches the
//! wire. This is the privacy backstop for Approach A (serde-typed events).

use serde_json::Value;

use crate::event::{Event, to_attributes};

/// A small default corpus of substrings that must never appear in any attribute
/// value. Apps SHOULD extend this with their own sensitive markers.
pub const FORBIDDEN_SUBSTRINGS: &[&str] = &[
    "@", "/Users/", "/home/", "C:\\", "http://", "https://", "SELECT ", "password", "secret",
];

/// Assert that no string attribute on `event` contains any `forbidden` substring.
/// Panics (test-only use) with the offending attribute and substring.
pub fn assert_no_forbidden<E: Event>(event: &E, forbidden: &[&str]) {
    let attrs = to_attributes(event).expect("event must serialize to scalar attributes");
    for (key, value) in &attrs {
        if let Value::String(s) = value {
            for f in forbidden {
                assert!(
                    !s.contains(f),
                    "attribute `{key}` = {s:?} leaked forbidden substring {f:?}"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{CommandInvoked, Outcome, Surface};

    #[test]
    fn clean_event_passes() {
        let e = CommandInvoked {
            command: "search".into(),
            duration_ms: 1,
            outcome: Outcome::Ok,
            surface: Surface::Cli,
        };
        assert_no_forbidden(&e, FORBIDDEN_SUBSTRINGS);
    }

    #[test]
    #[should_panic(expected = "leaked forbidden substring")]
    fn leaky_event_is_caught() {
        let e = CommandInvoked {
            command: "/Users/alice/secret".into(),
            duration_ms: 1,
            outcome: Outcome::Ok,
            surface: Surface::Cli,
        };
        assert_no_forbidden(&e, FORBIDDEN_SUBSTRINGS);
    }
}
```

- [ ] **Step 2: Re-export from lib.rs**

Add to `src/lib.rs`:

```rust
pub use canary::{FORBIDDEN_SUBSTRINGS, assert_no_forbidden};
```

- [ ] **Step 3: Write the integration canary suite for the kernel's common events**

Create `crates/gauge-telemetry/tests/canary_suite.rs`:

```rust
//! Probes every common event by stuffing a forbidden string into each
//! free-string field, proving the structural conversion carries no nested data
//! and that the harness catches leaks. (Closed-enum and numeric fields cannot
//! carry forbidden strings by construction.)

use gauge_telemetry::canary::{FORBIDDEN_SUBSTRINGS, assert_no_forbidden};
use gauge_telemetry::common::{CommandInvoked, ErrorEvent, Heartbeat, Install, Outcome, Surface, ToolCall};
use gauge_telemetry::env::EnvAttributes;

#[test]
fn common_events_with_clean_values_pass() {
    assert_no_forbidden(
        &CommandInvoked { command: "search".into(), duration_ms: 1, outcome: Outcome::Ok, surface: Surface::Cli },
        FORBIDDEN_SUBSTRINGS,
    );
    assert_no_forbidden(
        &ToolCall { tool: "search".into(), latency_ms: 1, result_count: 3, outcome: Outcome::Ok },
        FORBIDDEN_SUBSTRINGS,
    );
    assert_no_forbidden(&ErrorEvent { error_class: "timeout".into(), surface: Surface::Mcp }, FORBIDDEN_SUBSTRINGS);
    assert_no_forbidden(
        &Install { install_method: "brew".into(), env: EnvAttributes::default() },
        FORBIDDEN_SUBSTRINGS,
    );
    assert_no_forbidden(
        &Heartbeat { env: EnvAttributes { accel: Some("metal".into()), ..Default::default() } },
        FORBIDDEN_SUBSTRINGS,
    );
}
```

- [ ] **Step 4: Make `env` module's `EnvAttributes` reachable for the test**

Confirm `src/lib.rs` exposes `pub mod env;` and `pub mod common;` (it does from Task 0/2/3). Run:
`cargo test -p gauge-telemetry --test canary_suite`
Expected: PASS. Also run `cargo test -p gauge-telemetry canary::` for the unit tests.

- [ ] **Step 5: Commit**

```bash
git add crates/gauge-telemetry/src/canary.rs crates/gauge-telemetry/src/lib.rs crates/gauge-telemetry/tests/canary_suite.rs
git commit -m "feat(telemetry): canary harness + common-event canary suite"
```

---

## Task 11: Profile-conformance test

**Files:**
- Create: `crates/gauge-telemetry/tests/conformance.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/gauge-telemetry/tests/conformance.rs`. It exercises the full kernel emit path and proves the result satisfies the Gauge profile via `gauge_events::profile::validate_batch`:

```rust
//! End-to-end (no network): emit through the kernel, read the queue the way the
//! sender does, re-encode the OTLP batch, and prove it passes the Gauge profile
//! validator the server uses on ingest.

use gauge_events::profile::validate_batch;
use gauge_events::sender::queue::read_lines;
use gauge_events::sender::{QueuedEvent, SenderConfig, encode_batch};
use gauge_telemetry::Telemetry;
use gauge_telemetry::common::{CommandInvoked, Heartbeat, Outcome, Surface};
use gauge_telemetry::env::EnvAttributes;

#[test]
fn emitted_events_pass_the_gauge_profile() {
    let tmp = tempfile::tempdir().unwrap();
    let t = Telemetry::builder()
        .app("tome")
        .app_version("0.7.0")
        .endpoint("https://example.invalid")
        .install_id_path(tmp.path().join("id"))
        .config_enabled(true)
        .runtime_enabled(true)
        .build()
        .unwrap();

    t.emit(&CommandInvoked {
        command: "search".into(),
        duration_ms: 142,
        outcome: Outcome::Ok,
        surface: Surface::Cli,
    });
    t.emit(&Heartbeat {
        env: EnvAttributes { cpu_cores: Some(8), ram_gb: Some(16), accel: Some("metal".into()), ..Default::default() },
    });

    // Rebuild the batch the same way `drain` does.
    let queue = tmp.path().join("id.queue.jsonl");
    let events: Vec<QueuedEvent> = read_lines(&queue)
        .unwrap()
        .iter()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(events.len(), 2);

    // A SenderConfig identical in resource shape to the live one.
    let cfg = SenderConfig {
        endpoint: "https://example.invalid".into(),
        app: "tome".into(),
        app_version: "0.7.0".into(),
        install_id: uuid::Uuid::new_v4(),
        session_id: uuid::Uuid::new_v4(),
        os: gauge_telemetry::env::os_type(),
        arch: gauge_telemetry::env::host_arch(),
        queue_path: queue.clone(),
    };
    let req = encode_batch(&cfg, &events);

    let batch = validate_batch(&req, &["tome".to_string()]).expect("must validate");
    assert_eq!(batch.resource.app, "tome");
    assert_eq!(batch.events.len(), 2);
    assert!(batch.rejections.is_empty(), "no rejections: {:?}", batch.rejections);
    assert!(batch.events.iter().all(|e| e.event_name.starts_with("tome.")));
}
```

- [ ] **Step 2: Run it**

Run: `cargo test -p gauge-telemetry --test conformance`
Expected: PASS. If it fails on the `<app>.` prefix, confirm `emit` formats `format!("{}.{}", app, name)`.

- [ ] **Step 3: Commit**

```bash
git add crates/gauge-telemetry/tests/conformance.rs
git commit -m "test(telemetry): conformance against the Gauge profile validator"
```

---

## Task 12: SPEC.md + byte-pinned spec test

**Files:**
- Create: `crates/gauge-telemetry/SPEC.md`
- Create: `crates/gauge-telemetry/tests/spec_pin.rs`

- [ ] **Step 1: Write SPEC.md**

Create `crates/gauge-telemetry/SPEC.md`:

````markdown
# gauge-telemetry wire contract (v1)

`gauge-telemetry` emits the **Gauge OTLP profile** (see
`gauge-events/SPEC.md`). On top of that profile this crate guarantees:

- Event names are app-namespaced: the bare `Event::name()` is prefixed with
  `<service.name>.` (e.g. `command_invoked` → `tome.command_invoked`).
- Attribute values are scalars only (string / bool / int / double); `None`
  fields are omitted; nested values are rejected at emit.
- Quantities (`duration_ms`, `latency_ms`, `cpu_cores`, `ram_gb`, counts, rank)
  are sent as **raw integers** — bucketing happens at read time on the server.
- Environment attributes ride only on the low-frequency `install` / `heartbeat`
  events.

## Worked example

A `CommandInvoked { command: "search", duration_ms: 142, outcome: Ok,
surface: Cli }` for app `tome`, install `00000000-0000-4000-8000-000000000001`,
session `00000000-0000-4000-8000-000000000002`, at `timeUnixNano`
`1781430705123000000`, on `darwin`/`arm64`, encodes to the OTLP body pinned by
`tests/spec_pin.rs`.
````

- [ ] **Step 2: Write the byte-pinned test**

Create `crates/gauge-telemetry/tests/spec_pin.rs`:

```rust
//! Pins the exact OTLP body for the SPEC.md worked example. If this snapshot
//! changes, the wire contract changed — update SPEC.md deliberately.

use gauge_events::sender::{QueuedEvent, SenderConfig, encode_batch};

fn fixed_cfg(queue: std::path::PathBuf) -> SenderConfig {
    SenderConfig {
        endpoint: "https://example.invalid".into(),
        app: "tome".into(),
        app_version: "0.7.0".into(),
        install_id: uuid::uuid!("00000000-0000-4000-8000-000000000001"),
        session_id: uuid::uuid!("00000000-0000-4000-8000-000000000002"),
        os: "darwin".into(),
        arch: "arm64".into(),
        queue_path: queue,
    }
}

#[test]
fn command_invoked_otlp_body_is_pinned() {
    // The exact attributes `to_attributes(CommandInvoked{..})` produces.
    let mut attributes = serde_json::Map::new();
    attributes.insert("command".into(), serde_json::json!("search"));
    attributes.insert("duration_ms".into(), serde_json::json!(142));
    attributes.insert("outcome".into(), serde_json::json!("ok"));
    attributes.insert("surface".into(), serde_json::json!("cli"));

    let ev = QueuedEvent {
        event_name: "tome.command_invoked".into(),
        time_unix_nano: 1_781_430_705_123_000_000,
        attributes,
    };
    let tmp = tempfile::tempdir().unwrap();
    let req = encode_batch(&fixed_cfg(tmp.path().join("q.jsonl")), &[ev]);
    let body = serde_json::to_string_pretty(&req).unwrap();
    insta::assert_snapshot!(body);
}
```

- [ ] **Step 3: Generate and accept the snapshot**

Run: `cargo test -p gauge-telemetry --test spec_pin`
Expected: FAIL first run (new snapshot). Review the pending snapshot:
`cargo insta review` (accept it) — or set `INSTA_UPDATE=always` once.
Re-run: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/gauge-telemetry/SPEC.md crates/gauge-telemetry/tests/spec_pin.rs crates/gauge-telemetry/tests/snapshots
git commit -m "test(telemetry): SPEC.md + byte-pinned OTLP worked example"
```

---

## Task 13: PORTING.md (deliverable)

**Files:**
- Create: `crates/gauge-telemetry/PORTING.md`

- [ ] **Step 1: Write PORTING.md**

Create `crates/gauge-telemetry/PORTING.md` with the migration plan for both apps (this is a required deliverable per the spec; porting itself is a separate cycle):

````markdown
# Porting apps onto `gauge-telemetry`

This crate replaces an app's bespoke telemetry. Each migration is its own
project cycle; this document is the plan input.

## Common steps (any app)

1. Add `gauge-telemetry` as a dependency (published on crates.io).
2. Build a `Telemetry` once at startup via `Telemetry::builder()`:
   `.app(<service.name>)`, `.app_version(env!("CARGO_PKG_VERSION"))`,
   `.endpoint(<compiled-in HTTPS endpoint>)`, `.install_id_path(<app path>)`,
   `.app_env_var(<APP>_TELEMETRY)`, `.config_enabled(<from config>)`,
   `.runtime_enabled(<from runtime toggle>)`, optional `.accel(<backend>)`.
3. Map events: use the common events where they fit; define app-specific events
   as `#[derive(Serialize)]` structs implementing `Event` with a bare `name()`.
4. Replace all client-side buckets with **raw integer** fields (server buckets at
   read time — see gauge#22).
5. Choose a flush trigger: CLIs → wire a hidden flush subcommand to
   `telemetry.run_flush()` and call `telemetry.spawn_detached_flush()` at exit
   (or call `flush_blocking`); long-running → `Flusher::start(...)` at boot.
6. Wire `telemetry.reset()` to the app's `reset` subcommand and the runtime
   toggle to `.runtime_enabled(false)`.
7. Add a canary test using `gauge_telemetry::canary::assert_no_forbidden` over
   every event, extending `FORBIDDEN_SUBSTRINGS` with app-specific markers.
8. Ensure `<service.name>` is on the server `GAUGE_APP_ALLOWLIST`.

## tome

- Wire format JSON → OTLP (handled by the kernel).
- Envelope remap is automatic: `macos→darwin`, `x86_64→amd64`.
- `os=macos/linux`, `arch=x86_64/aarch64` strings disappear (kernel emits the
  profile vocabulary).
- All `count`/`rank`/`load`/`latency` buckets → raw integers.
- The `catalog.<id>.*` attributed stream must be renamed under the `tome.`
  namespace (e.g. `tome.catalog_*`) to satisfy the `<app>.` prefix rule; the
  source allowlist + canonicalization logic stays in tome and feeds custom
  `Event` types.
- 18 anonymous + 6 attributed events → map to common events + custom events.
- `schema_version` / `sample_rate` / `calling_harness`: carry as optional
  attributes on the relevant events (no global envelope field).
- Retire `src/telemetry/`.

## midnight-manual

- Wire format JSON-array → OTLP (handled by the kernel).
- In-memory buffer + jittered backoff → the kernel's disk queue + `Flusher`
  (drop the backoff; at-least-once comes from the queue).
- Server-side CHECK-constraint schema → kernel scalar validation + a canary
  test.
- **Decision to confirm:** retire mn's own `/v1/telemetry/events` server path in
  favour of Gauge `/v1/logs`.
- mn does not persist an install UUID today — the kernel now mints/persists one.
- Map the 7 event types (`mcp_tool_call`, `rerank`, `cli_command`,
  `ingest_complete`, `pull_models`, `mcp_startup`, `mcp_shutdown`) to common
  events (`ToolCall`, `CommandInvoked`, lifecycle) + custom events.
````

- [ ] **Step 2: Commit**

```bash
git add crates/gauge-telemetry/PORTING.md
git commit -m "docs(telemetry): PORTING.md migration plan for tome + mn"
```

---

## Task 14: Crate docs, full test run, and publish readiness

**Files:**
- Modify: `crates/gauge-telemetry/src/lib.rs`
- Modify: `release-plz.toml` (register the new crate, if needed)

- [ ] **Step 1: Flesh out crate-level docs with a usage example**

Replace the top of `src/lib.rs` doc comment with a runnable-shaped example (use `no_run`/`ignore` as appropriate):

```rust
//! `gauge-telemetry` — privacy-first telemetry client kernel for Gauge.
//!
//! ```ignore
//! use gauge_telemetry::Telemetry;
//! use gauge_telemetry::common::{CommandInvoked, Outcome, Surface};
//!
//! let telemetry = Telemetry::builder()
//!     .app("tome")
//!     .app_version(env!("CARGO_PKG_VERSION"))
//!     .endpoint("https://gauge.example/")
//!     .install_id_path(dirs_path())
//!     .app_env_var("TOME_TELEMETRY")
//!     .config_enabled(true)
//!     .runtime_enabled(true)
//!     .build()?;
//!
//! telemetry.emit(&CommandInvoked {
//!     command: "search".into(), duration_ms: 142,
//!     outcome: Outcome::Ok, surface: Surface::Cli,
//! });
//! telemetry.flush_blocking(std::time::Duration::from_secs(2));
//! # Ok::<(), gauge_telemetry::BuildError>(())
//! ```
```

- [ ] **Step 2: Run the full crate test suite + lints**

Run:
```bash
cargo test -p gauge-telemetry
cargo clippy -p gauge-telemetry -- -D warnings
cargo fmt -p gauge-telemetry -- --check
```
Expected: all tests PASS; no clippy warnings; formatting clean.

- [ ] **Step 3: Verify it builds as part of the workspace and docs render**

Run:
```bash
cargo build --workspace
cargo doc -p gauge-telemetry --no-deps
```
Expected: workspace builds; docs build without warnings.

- [ ] **Step 4: Register the crate for release automation**

Check `release-plz.toml`. The repo publishes all workspace crates via release-plz; confirm `gauge-telemetry` is picked up (release-plz publishes all `members` by default). If the config pins an explicit per-package list, add a `[[package]]` entry for `gauge-telemetry` mirroring `gauge-events`. If it relies on workspace auto-discovery, no change is needed — note this in the commit message.

- [ ] **Step 5: Commit**

```bash
git add crates/gauge-telemetry/src/lib.rs release-plz.toml
git commit -m "docs(telemetry): crate docs + publish readiness"
```

- [ ] **Step 6: Push the branch**

```bash
git push -u origin feat/gauge-telemetry-kernel
```

---

## Self-Review (completed by plan author)

**1. Spec coverage** — every spec section maps to a task:
- §4 crate shape / layering → Task 0. §5 API (Event, common, builder, emit) → Tasks 1, 3, 6.
- §6 identity → Task 4. §7 consent (precedence, `GAUGE_TELEMETRY_DISABLE`, grace, reset) → Tasks 5, 6.
- §8 envelope + env attributes (locked set; `in_container` excluded) → Tasks 2, 3.
- §9 raw-int quantities → enforced by event types (Tasks 3) + documented (SPEC.md, Task 12).
- §10 delivery (hot-path append, disk queue, detached + background + blocking, sync core, failure handling) → Tasks 6–9.
- §11 privacy/validation (non-fatal emit, scalar-only, limits, canary) → Tasks 1, 10.
- §12 testing (unit, conformance, canary, spec-pin) → Tasks 1–12.
- §13 PORTING.md → Task 13. §14 deps/out-of-scope → Tasks 0, 14; server bucketing is gauge#22, not in this plan.

**2. Placeholder scan** — no TBD/TODO/"handle errors"/"similar to Task N". One intentional refactor is spelled out with full code (Task 6 Step 2 fixes the `Inner.install_id_path` field rather than leaving a stub).

**3. Type consistency** — `Event::name() -> Cow<'_, str>`, `to_attributes` signature, `EmitError`, `SenderConfig` field names (`endpoint`/`app`/`app_version`/`install_id`/`session_id`/`os`/`arch`/`queue_path`), `Telemetry(Option<Inner>)`, `Inner` fields, `consent::resolve`/`within_grace`/`is_ci`, `Flusher::start(&Telemetry, Duration, u64)`, and the re-exports are consistent across tasks. The `Telemetry::inner()` accessor (Task 8 Step 2) backs `Flusher`. `EnvAttributes` is defined once in `env.rs` and consumed by `common.rs`/tests.

Known risk flagged inline (not a placeholder): the `sysinfo` API surface in `env.rs` (Task 2) must be verified against the resolved 0.33 version; the tests don't assert its values, so a signature fix is localized.

---

## Execution Handoff

**Plan complete and saved to `docs/superpowers/plans/2026-06-17-gauge-telemetry-kernel.md`. Two execution options:**

**1. Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration.

**2. Inline Execution** — Execute tasks in this session using executing-plans, batch execution with checkpoints.

**Which approach?**
