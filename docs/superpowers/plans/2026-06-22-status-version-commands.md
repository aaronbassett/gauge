# `gauge status` and `gauge version` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a server-aware `gauge status` command (sparkline art + info panel, `--json`, Tome-parity exit codes) and a bare `gauge version` subcommand + `--version`/`-V` flag.

**Architecture:** Two new modules in the `gauge-client` crate — `status/` (report types, async assembly, classification, human+JSON rendering) and `status/art.rs` (sparkline) — plus a dependency-free `term.rs` for TTY/color/width helpers (gauge has no plain-text color helper today; color currently lives only in the ratatui TUI). `status` reuses the existing `ApiClient` for the authed `/v1/meta` probe and adds unauthed `healthz`/`readyz` probes + a configurable timeout. Every probe is best-effort: failures become report fields, never panics or early errors.

**Tech Stack:** Rust 2024, `clap` (derive), `reqwest`, `serde`/`serde_json`, `time`, `ratatui::style::Color` (palette reuse only), `wiremock` + `tempfile` + `insta` (tests).

**Spec:** `docs/superpowers/specs/2026-06-22-status-version-commands-design.md`

---

## File Structure

| File | Responsibility |
|---|---|
| `crates/gauge/src/term.rs` (create) | `stdout_is_tty()`, `color_enabled()` (TTY && `NO_COLOR` unset), `term_width()`, and semantic ANSI wrappers (`bold`/`dim`/`label`/`green`/`yellow`/`red`). No ratatui dependency. |
| `crates/gauge/src/status/mod.rs` (create) | `StatusReport` types, `assemble_report()` (async, infallible), `classify()`, formatting helpers, `emit()` (human panel + JSON). |
| `crates/gauge/src/status/art.rs` (create) | Sparkline-wave art: `ART_WIDTH`, `sparkline(accents)`, per-column palette paint, `ratatui::Color → ANSI`. |
| `crates/gauge/src/api.rs` (modify) | Add `from_config_with_timeout()`, unauthed `healthz()`/`readyz()` probes (raw GET, status-only). |
| `crates/gauge/src/lib.rs` (modify) | `pub mod status;` + `pub mod term;`. |
| `crates/gauge/src/main.rs` (modify) | `Status { json }` + `Version` subcommands; `--version`/`-V` pre-parse hook. |
| `crates/gauge/tests/status.rs` (create) | wiremock integration: healthy / unhealthy / degraded; `--version` + `version` bare-output; `--json` snapshot. |
| `README.md` (modify) | Document the two commands under the client section. |

**Visibility note:** all new modules/types/fns are declared `pub` (e.g. `pub mod art;`). The crate builds with `clippy -D warnings`; `pub` items at the crate root are reachable, so the incremental commits below don't trip `dead_code` before later tasks wire them up.

---

## Task 1: `term.rs` — TTY / color / width helpers

**Files:**
- Create: `crates/gauge/src/term.rs`
- Modify: `crates/gauge/src/lib.rs`

- [ ] **Step 1: Declare the module**

In `crates/gauge/src/lib.rs`, add **one line** — `pub mod term;` — after `pub mod query_cmd;` (the list is alphabetical; `pub mod status;` is added separately in Task 2, since `status/mod.rs` doesn't exist yet). The file becomes:

```rust
pub mod api;
pub mod config;
pub mod error;
pub mod keys;
pub mod mcp;
pub mod paths;
pub mod query_cmd;
pub mod term;
pub mod tui;
```

- [ ] **Step 2: Write the failing test**

Create `crates/gauge/src/term.rs` with only the test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_disabled_in_non_tty_test_process() {
        // The test process has no TTY on stdout, so colour is off and the
        // wrappers must return their input unchanged (no ANSI escapes).
        assert_eq!(bold("hi"), "hi");
        assert_eq!(green("ok"), "ok");
        assert!(!color_enabled());
    }

    #[test]
    fn term_width_reads_columns_env_else_80() {
        unsafe { std::env::set_var("COLUMNS", "123") };
        assert_eq!(term_width(), 123);
        unsafe { std::env::remove_var("COLUMNS") };
        assert_eq!(term_width(), 80);
    }
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p gauge-client --lib term::`
Expected: FAIL — compile error, `bold`/`green`/`color_enabled`/`term_width` not found.

- [ ] **Step 4: Write the implementation**

Prepend to `crates/gauge/src/term.rs` (above the test module):

```rust
//! Plain-text terminal helpers for non-TUI command output: TTY detection,
//! colour gating (TTY && no `NO_COLOR`), terminal width, and semantic ANSI
//! wrappers. Colour lives in the ratatui TUI for the dashboard; this is the
//! CLI-side equivalent for `gauge status`.

use std::io::IsTerminal as _;

const RESET: &str = "\x1b[0m";

/// True when stdout is an interactive terminal.
pub fn stdout_is_tty() -> bool {
    std::io::stdout().is_terminal()
}

/// Colour is enabled only on a TTY and when `NO_COLOR` is unset
/// (https://no-color.org/).
pub fn color_enabled() -> bool {
    stdout_is_tty() && std::env::var_os("NO_COLOR").is_none()
}

/// Best-effort terminal width: `$COLUMNS` if a positive integer, else 80.
pub fn term_width() -> usize {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok())
        .filter(|w| *w > 0)
        .unwrap_or(80)
}

fn wrap(code: &str, s: &str) -> String {
    if color_enabled() {
        format!("\x1b[{code}m{s}{RESET}")
    } else {
        s.to_string()
    }
}

pub fn bold(s: &str) -> String {
    wrap("1", s)
}
pub fn dim(s: &str) -> String {
    wrap("2", s)
}
/// Field-label colour (cyan) for the status panel keys.
pub fn label(s: &str) -> String {
    wrap("36", s)
}
pub fn green(s: &str) -> String {
    wrap("32", s)
}
pub fn yellow(s: &str) -> String {
    wrap("33", s)
}
pub fn red(s: &str) -> String {
    wrap("31", s)
}
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test -p gauge-client --lib term::`
Expected: PASS (2 tests).

- [ ] **Step 6: Commit**

```bash
git add crates/gauge/src/term.rs crates/gauge/src/lib.rs
git commit -m "feat(gauge): add term.rs TTY/colour/width helpers"
```

---

## Task 2: `status/art.rs` — sparkline-wave art

**Files:**
- Create: `crates/gauge/src/status/mod.rs`
- Create: `crates/gauge/src/status/art.rs`
- Modify: `crates/gauge/src/lib.rs`

- [ ] **Step 1: Declare the module**

In `crates/gauge/src/lib.rs`, add `pub mod status;` (alphabetical, before `term`). The list now matches the Task 1 target.

Create `crates/gauge/src/status/mod.rs` with exactly:

```rust
//! `gauge status` — client/server health + data overview.

pub mod art;
```

- [ ] **Step 2: Write the failing test**

Create `crates/gauge/src/status/art.rs` with the test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    #[test]
    fn lines_are_exactly_art_width_visible() {
        // No accents → no ANSI, so char count == visible width.
        let lines = sparkline(&[]);
        assert_eq!(lines.len(), 5, "top + 3 wave rows + bottom");
        for l in &lines {
            assert_eq!(l.chars().count(), ART_WIDTH, "line {l:?} not ART_WIDTH wide");
        }
    }

    #[test]
    fn fit_pads_and_truncates_to_interior() {
        assert_eq!(fit("ab").chars().count(), INTERIOR);
        assert_eq!(fit(&"x".repeat(100)).chars().count(), INTERIOR);
    }

    #[test]
    fn fg_maps_rgb_to_truecolor_escape() {
        assert_eq!(fg(Color::Rgb(1, 2, 3)), "\x1b[38;2;1;2;3m");
        assert_eq!(fg(Color::Reset), "");
    }
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p gauge-client --lib status::art::`
Expected: FAIL — `sparkline`/`fit`/`fg`/`ART_WIDTH`/`INTERIOR` not found.

- [ ] **Step 4: Write the implementation**

Prepend to `crates/gauge/src/status/art.rs` (above the tests):

```rust
//! The `gauge status` sparkline-wave art. Original art (no third-party
//! source). Each returned line is exactly `ART_WIDTH` VISIBLE columns wide so
//! the column zipper can place the panel at a stable offset without measuring
//! ANSI escapes. Non-space glyphs cycle through the supplied palette accents.

use ratatui::style::Color;

use crate::term;

/// Visible width of the frame interior (between the `│` borders).
pub const INTERIOR: usize = 23;
/// Visible width of every art line: `│` + interior + `│`.
pub const ART_WIDTH: usize = INTERIOR + 2;

// Wave rows. Exact source spacing is forgiving — `fit` pads/truncates each to
// INTERIOR before framing. Tune visually; widths are normalised in code.
const ROWS: [&str; 3] = [
    "        ╱╲    ╱╲        ",
    "  ╱╲╱╲╱   ╲╱╲╱   ╲╱╲    ",
    " ╱╲              ╲╱╲    ",
];

/// Pad (with spaces) or truncate `row` to exactly `INTERIOR` visible chars.
/// Char-based so multi-byte box-drawing glyphs each count as one column.
pub fn fit(row: &str) -> String {
    let mut s: String = row.chars().take(INTERIOR).collect();
    let n = s.chars().count();
    if n < INTERIOR {
        s.push_str(&" ".repeat(INTERIOR - n));
    }
    s
}

/// ANSI foreground escape for a palette colour, or `""` for colours we don't
/// map (e.g. `Reset`/`DarkGray`) — the glyph then renders in the default fg.
pub fn fg(c: Color) -> String {
    match c {
        Color::Rgb(r, g, b) => format!("\x1b[38;2;{r};{g};{b}m"),
        Color::Red => "\x1b[31m".into(),
        Color::Green => "\x1b[32m".into(),
        Color::Yellow => "\x1b[33m".into(),
        Color::Blue => "\x1b[34m".into(),
        Color::Magenta => "\x1b[35m".into(),
        Color::Cyan => "\x1b[36m".into(),
        Color::LightBlue => "\x1b[94m".into(),
        _ => String::new(),
    }
}

/// Colour each non-space glyph by its position, cycling through `accents`.
/// Spaces pass through. Plain (no ANSI) when colour is disabled or there are
/// no accents — which keeps the visible width equal to the input width.
fn paint(row: &str, accents: &[Color]) -> String {
    if !term::color_enabled() || accents.is_empty() {
        return row.to_string();
    }
    let mut out = String::new();
    let mut i = 0usize;
    for ch in row.chars() {
        if ch == ' ' {
            out.push(' ');
            continue;
        }
        out.push_str(&fg(accents[i % accents.len()]));
        out.push(ch);
        out.push_str("\x1b[0m");
        i += 1;
    }
    out
}

/// The framed sparkline: top border, 3 wave rows, bottom border — five lines,
/// each `ART_WIDTH` visible columns wide, top-aligned.
pub fn sparkline(accents: &[Color]) -> Vec<String> {
    let bar = "─".repeat(INTERIOR);
    let mut lines = vec![format!("┌{bar}┐")];
    for row in ROWS {
        lines.push(format!("│{}│", paint(&fit(row), accents)));
    }
    lines.push(format!("└{bar}┘"));
    lines
}
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test -p gauge-client --lib status::art::`
Expected: PASS (3 tests).

- [ ] **Step 6: Commit**

```bash
git add crates/gauge/src/status/ crates/gauge/src/lib.rs
git commit -m "feat(gauge): add status sparkline-wave art"
```

---

## Task 3: `ApiClient` server probes (`healthz` / `readyz` / configurable timeout)

**Files:**
- Modify: `crates/gauge/src/api.rs`
- Test: `crates/gauge/tests/api.rs`

- [ ] **Step 1: Write the failing test**

Append to `crates/gauge/tests/api.rs` (it already has `env_lock`, `setup`, wiremock imports):

```rust
#[tokio::test]
async fn healthz_ok_and_readyz_failure_are_distinguished() {
    let _g = env_lock().await;
    let tmp = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/healthz"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/readyz"))
        .respond_with(ResponseTemplate::new(503).set_body_string("db down"))
        .mount(&server)
        .await;
    let api = setup(&tmp, &server.uri());

    assert!(api.healthz().await.is_ok());
    assert!(api.readyz().await.is_err());
    unsafe { std::env::remove_var("GAUGE_CONFIG_DIR") };
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p gauge-client --test api healthz_ok_and_readyz_failure_are_distinguished`
Expected: FAIL — `healthz`/`readyz` methods not found.

- [ ] **Step 3: Write the implementation**

In `crates/gauge/src/api.rs`, replace the existing `from_config` constructor:

```rust
    pub fn from_config(cfg: &ClientConfig) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("reqwest client"),
            base: cfg.server_url.clone(),
            user_id: cfg.user_id.clone(),
        }
    }
```

with a timeout-parameterised pair:

```rust
    pub fn from_config(cfg: &ClientConfig) -> Self {
        Self::from_config_with_timeout(cfg, std::time::Duration::from_secs(10))
    }

    /// Like [`from_config`](Self::from_config) but with a caller-chosen request
    /// timeout. `gauge status` uses a short timeout so the health probe stays
    /// snappy when the server is down.
    pub fn from_config_with_timeout(cfg: &ClientConfig, timeout: std::time::Duration) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(timeout)
                .build()
                .expect("reqwest client"),
            base: cfg.server_url.clone(),
            user_id: cfg.user_id.clone(),
        }
    }
```

Then add the two unauthed probes plus their shared raw-GET helper. Place them right after the `meta` method:

```rust
    /// Unauthed liveness probe: `GET /healthz`. Ok ⇒ reachable.
    pub async fn healthz(&self) -> Result<(), ClientError> {
        self.get_ok("/healthz").await
    }

    /// Unauthed readiness probe: `GET /readyz` (server checks its DB). Ok ⇒
    /// the server can serve queries.
    pub async fn readyz(&self) -> Result<(), ClientError> {
        self.get_ok("/readyz").await
    }

    /// Raw GET that only inspects the status line — the health endpoints reply
    /// with a bare `ok` body, which is not JSON, so this must NOT go through
    /// `handle` (which deserializes). Any transport error or non-2xx maps to a
    /// `ClientError` whose `Display` is the human-readable reason.
    async fn get_ok(&self, path: &str) -> Result<(), ClientError> {
        let resp = self
            .http
            .get(format!("{}{path}", self.base))
            .send()
            .await
            .map_err(|e| ClientError::Http(e.to_string()))?;
        let status = resp.status().as_u16();
        if (200..300).contains(&status) {
            Ok(())
        } else {
            Err(ClientError::Http(format!("HTTP {status}")))
        }
    }
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p gauge-client --test api healthz_ok_and_readyz_failure_are_distinguished`
Expected: PASS.

- [ ] **Step 5: Run the full api test file (no regressions)**

Run: `cargo test -p gauge-client --test api`
Expected: PASS (all pre-existing tests + the new one).

- [ ] **Step 6: Commit**

```bash
git add crates/gauge/src/api.rs crates/gauge/tests/api.rs
git commit -m "feat(gauge): add ApiClient healthz/readyz probes + timeout ctor"
```

---

## Task 4: `status` report types, assembly, classification, formatters

**Files:**
- Modify: `crates/gauge/src/status/mod.rs`

- [ ] **Step 1: Write the failing test**

Append a test module to `crates/gauge/src/status/mod.rs`:

```rust
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

    #[tokio::test]
    async fn unconfigured_is_unhealthy() {
        let report = assemble_report(Err(crate::error::ClientError::NoConfigDir)).await;
        assert_eq!(report.overall, Overall::Unhealthy);
        assert!(!report.client.config_loaded);
        assert!(!report.server.reachable);
        assert!(!report.data.available);
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p gauge-client --lib status::tests`
Expected: FAIL — types/functions not found.

- [ ] **Step 3: Write the implementation**

In `crates/gauge/src/status/mod.rs`, insert ABOVE the `#[cfg(test)] mod tests` block (and below `pub mod art;`):

```rust
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

// ---- Humanizers ------------------------------------------------------------

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
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p gauge-client --lib status::tests`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/gauge/src/status/mod.rs
git commit -m "feat(gauge): status report types, assembly, classification"
```

---

## Task 5: `status` rendering — JSON + human panel/zipper

**Files:**
- Modify: `crates/gauge/src/status/mod.rs`

- [ ] **Step 1: Write the failing test**

Add these tests inside the existing `#[cfg(test)] mod tests` block in `status/mod.rs`:

```rust
    fn healthy_report() -> StatusReport {
        StatusReport {
            gauge: "9.9.9".into(),
            client: ClientStatus {
                config_path: "/home/x/.config/gauge/config.toml".into(),
                config_loaded: true,
                server_url: "https://gauge.example".into(),
                user_id: "aaron".into(),
                key_present: true,
                token: TokenStatus { present: true, valid: true, expires_at: Some(10), expires_in_secs: Some(2_460) },
            },
            server: ServerStatus { endpoint: "https://gauge.example".into(), reachable: true, db_ready: true, error: None },
            data: DataStatus { available: true, apps: 3, total_events: 1_200_000, last_event: Some("2026-06-22T10:25:00Z".into()), per_app: vec![], error: None },
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
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p gauge-client --lib status::tests`
Expected: FAIL — `human_panel` not found.

- [ ] **Step 3: Write the implementation**

In `status/mod.rs`, add two lines to the `use` block at the top — `use std::io::Write as _;` and `use crate::term;` (the rendering code below calls `term::green(...)`, `term::label(...)`, etc.; without the import it won't resolve and `clippy -D warnings` would also reject an unused import if added earlier in Task 4, which is why it lands here where it's first used). Then append the rendering functions ABOVE the test module:

```rust
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
    if term::color_enabled() { term::green("✓") } else { "[ok]".to_string() }
}
fn warn_mark() -> String {
    if term::color_enabled() { term::yellow("⚠") } else { "[warn]".to_string() }
}
fn fail_mark() -> String {
    if term::color_enabled() { term::red("✗") } else { "[fail]".to_string() }
}

fn token_line(t: &TokenStatus) -> String {
    if !t.present {
        return format!("{} none cached", fail_mark());
    }
    if t.valid {
        format!("{} valid · expires in {}", ok_mark(), human_duration(t.expires_in_secs.unwrap_or(0)))
    } else {
        let expired = t.expires_in_secs.map(|s| s <= 0).unwrap_or(true);
        format!("{} {}", warn_mark(), if expired { "expired" } else { "stale" })
    }
}

fn reachable_line(s: &ServerStatus) -> String {
    if !s.reachable {
        return format!("{} unreachable ({})", fail_mark(), s.error.as_deref().unwrap_or("no response"));
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
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() && path.starts_with(&home) {
            return path.replacen(&home, "~", 1);
        }
    }
    path.to_string()
}

fn rel_from_rfc3339(s: &str) -> String {
    match OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339) {
        Ok(dt) => relative_time(dt.unix_timestamp(), OffsetDateTime::now_utc().unix_timestamp()),
        Err(_) => s.to_string(),
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
        if r.client.user_id.is_empty() { dash.clone() } else { r.client.user_id.clone() }
    ));
    lines.push(format!(
        "{} {}",
        key("Key:"),
        if r.client.key_present { format!("{} present", ok_mark()) } else { format!("{} missing", fail_mark()) }
    ));
    lines.push(format!("{} {}", key("Token:"), token_line(&r.client.token)));
    lines.push(String::new());

    lines.push(term::dim("Server"));
    lines.push(format!(
        "{} {}",
        key("Endpoint:"),
        if r.client.server_url.is_empty() { dash.clone() } else { r.client.server_url.clone() }
    ));
    lines.push(format!("{} {}", key("Reachable:"), reachable_line(&r.server)));
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
            r.data.last_event.as_deref().map(rel_from_rfc3339).unwrap_or(dash)
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
    let show_art =
        term::stdout_is_tty() && term::term_width() >= art::ART_WIDTH + GAP + PANEL_MIN;

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
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p gauge-client --lib status::tests`
Expected: PASS (7 tests total in the module).

- [ ] **Step 5: Commit**

```bash
git add crates/gauge/src/status/mod.rs
git commit -m "feat(gauge): status JSON + human panel rendering"
```

---

## Task 6: Wire `status` + `version` into the CLI

**Files:**
- Modify: `crates/gauge/src/main.rs`

- [ ] **Step 1: Add the `--version`/`-V` pre-parse hook**

In `crates/gauge/src/main.rs`, make `main` intercept the version flag before `Cli::parse()`. Replace the start of `main`:

```rust
#[tokio::main]
async fn main() {
    let cli = Cli::parse();
```

with:

```rust
#[tokio::main]
async fn main() {
    // `--version` / `-V` print only the bare version, before clap dispatch.
    let raw: Vec<String> = std::env::args().collect();
    if raw.iter().skip(1).any(|a| a == "--version" || a == "-V") {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return;
    }

    let cli = Cli::parse();
```

- [ ] **Step 2: Add the subcommand variants**

In the `Cmd` enum, add two variants (after `Mcp`):

```rust
    /// Show client/server status and a data overview
    Status {
        /// Emit machine-readable JSON instead of the human panel
        #[arg(long)]
        json: bool,
    },
    /// Print the gauge version and exit
    Version,
```

- [ ] **Step 3: Add the dispatch arms**

In the `match cli.cmd { ... }` block, add arms (after the `Mcp` arm, before the closing `}`):

```rust
        Cmd::Status { json } => {
            let report =
                gauge::status::assemble_report(gauge::config::ClientConfig::load()).await;
            gauge::status::emit(&report, json);
            let code = report.overall.exit_code();
            if code != 0 {
                std::process::exit(code);
            }
            Ok(())
        }
        Cmd::Version => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
```

- [ ] **Step 4: Verify it compiles and runs**

Run: `cargo run -p gauge-client -- --version`
Expected: prints exactly the workspace version (e.g. `0.3.0`) and nothing else.

Run: `cargo run -p gauge-client -- version`
Expected: identical single-line output.

Run: `GAUGE_CONFIG_DIR=/tmp/gauge-empty cargo run -p gauge-client -- status; echo "exit=$?"`
Expected: a panel reporting `Config: missing …` / `Overall: [fail]/✗ unhealthy` (or colored on a TTY), then `exit=1`.

- [ ] **Step 5: Commit**

```bash
git add crates/gauge/src/main.rs
git commit -m "feat(gauge): wire status and version subcommands + --version flag"
```

---

## Task 7: Integration tests (`tests/status.rs`)

**Files:**
- Create: `crates/gauge/tests/status.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/gauge/tests/status.rs`:

```rust
//! Integration tests for `gauge status` (network probes via wiremock) and the
//! bare `--version` / `version` output (via the built binary).

use std::process::Command;
use std::sync::OnceLock;

use gauge::config::ClientConfig;
use gauge::status::{Overall, assemble_report};
use tokio::sync::{Mutex, MutexGuard};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn env_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().await
}

fn cfg(uri: &str) -> ClientConfig {
    ClientConfig { server_url: uri.trim_end_matches('/').into(), user_id: "alice".into() }
}

async fn mock_health(server: &MockServer, healthz: u16, readyz: u16) {
    Mock::given(method("GET"))
        .and(path("/healthz"))
        .respond_with(ResponseTemplate::new(healthz).set_body_string("ok"))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/readyz"))
        .respond_with(ResponseTemplate::new(readyz).set_body_string("ok"))
        .mount(server)
        .await;
}

async fn mock_auth(server: &MockServer) {
    use base64::Engine as _;
    let nonce_b64 = base64::engine::general_purpose::STANDARD_NO_PAD.encode([9u8; 32]);
    Mock::given(method("POST"))
        .and(path("/v1/auth/challenge"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "challenge_id": "00000000-0000-4000-8000-000000000001",
            "nonce_b64": nonce_b64,
            "expires_in_s": 60
        })))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/auth/verify"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "token": "test-token",
            "user_id": "alice",
            "expires_at": time::OffsetDateTime::now_utc().unix_timestamp() + 3600
        })))
        .mount(server)
        .await;
}

async fn mock_meta(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/v1/meta"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "apps": [{
                "app": "tome",
                "event_names": ["command"],
                "attribute_keys": ["name"],
                "numeric_attribute_keys": [],
                "first_event": "2026-06-01T00:00:00Z",
                "last_event": "2026-06-22T10:25:00Z",
                "total_events": 1200000
            }]
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn healthy_when_server_up_and_authed() {
    let _g = env_lock().await;
    let tmp = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("GAUGE_CONFIG_DIR", tmp.path()) };
    gauge::keys::generate("alice").unwrap();

    let server = MockServer::start().await;
    mock_health(&server, 200, 200).await;
    mock_auth(&server).await;
    mock_meta(&server).await;

    let report = assemble_report(Ok(cfg(&server.uri()))).await;
    assert_eq!(report.overall, Overall::Healthy);
    assert!(report.server.reachable && report.server.db_ready);
    assert!(report.data.available);
    assert_eq!(report.data.apps, 1);
    assert_eq!(report.data.total_events, 1_200_000);

    unsafe { std::env::remove_var("GAUGE_CONFIG_DIR") };
}

#[tokio::test]
async fn unhealthy_when_server_down() {
    let _g = env_lock().await;
    let tmp = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("GAUGE_CONFIG_DIR", tmp.path()) };

    let server = MockServer::start().await;
    mock_health(&server, 503, 503).await;

    let report = assemble_report(Ok(cfg(&server.uri()))).await;
    assert_eq!(report.overall, Overall::Unhealthy);
    assert!(!report.server.reachable);

    unsafe { std::env::remove_var("GAUGE_CONFIG_DIR") };
}

#[tokio::test]
async fn degraded_when_server_up_but_no_key() {
    let _g = env_lock().await;
    let tmp = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("GAUGE_CONFIG_DIR", tmp.path()) };
    // No key generated → /v1/meta login fails → data unavailable.

    let server = MockServer::start().await;
    mock_health(&server, 200, 200).await;

    let report = assemble_report(Ok(cfg(&server.uri()))).await;
    assert_eq!(report.overall, Overall::Degraded);
    assert!(report.server.reachable && report.server.db_ready);
    assert!(!report.data.available);

    unsafe { std::env::remove_var("GAUGE_CONFIG_DIR") };
}

#[test]
fn version_flag_and_subcommand_print_bare_version() {
    let bin = env!("CARGO_BIN_EXE_gauge");
    for args in [vec!["--version"], vec!["-V"], vec!["version"]] {
        let out = Command::new(bin).args(&args).output().unwrap();
        assert!(out.status.success(), "args {args:?} exited non-zero");
        let stdout = String::from_utf8(out.stdout).unwrap();
        assert_eq!(stdout.trim_end(), env!("CARGO_PKG_VERSION"), "args {args:?}");
        assert!(out.stderr.is_empty(), "args {args:?} wrote to stderr");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail (then pass)**

Run: `cargo test -p gauge-client --test status`
Expected: PASS once the binary builds (the `version_*` test depends on `CARGO_BIN_EXE_gauge`, which cargo provides to integration tests). If you run it before Task 6 is committed, the version test fails — Task 6 is a prerequisite.

- [ ] **Step 3: Commit**

```bash
git add crates/gauge/tests/status.rs
git commit -m "test(gauge): status integration + bare version output"
```

---

## Task 8: Document the commands + final verification

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Add a docs subsection**

In `README.md`, find the client section header `## The client (`gauge`)`. Immediately under it (before the next `###`/`##`), add:

````markdown
### `gauge status` and `gauge version`

```console
$ gauge status            # health panel: client config, server reachability, data overview
$ gauge status --json     # same report as structured JSON
$ gauge version           # prints just the version, e.g. 0.3.0
$ gauge --version         # identical (also -V)
```

`status` is read-only and degrades gracefully: an unreachable server or missing
credentials become fields in the report rather than errors. Exit code is `0`
when healthy and `1` when degraded or unhealthy, so it is safe to gate scripts
on `gauge status`.
````

- [ ] **Step 2: Commit the docs**

```bash
git add README.md
git commit -m "docs(gauge): document status and version commands"
```

- [ ] **Step 3: Full verification — format, lint, test**

Run each and confirm clean output:

```bash
cargo fmt --all
cargo clippy -p gauge-client --all-targets -- -D warnings
cargo test -p gauge-client
```

Expected: `fmt` makes no changes (or only trivial ones — commit them if so); `clippy` reports zero warnings; all tests pass (lib unit tests + `api`, `status`, and the pre-existing `config`/`keys`/`tui_*` integration suites).

- [ ] **Step 4: Manual smoke test on a real TTY (art + colour)**

```bash
cargo run -p gauge-client -- status
```

Expected (against your configured server): the sparkline box on the left in palette colors, the info panel on the right, ending in a colored `Overall:` line. Pipe it to confirm graceful downgrade: `cargo run -p gauge-client -- status | cat` should show the panel only (no art, no ANSI).

- [ ] **Step 5: Commit any formatting changes**

```bash
git add -A
git commit -m "style(gauge): cargo fmt" --allow-empty
```

---

## Self-Review notes (for the implementer)

- **Spec coverage:** version subcommand + `--version`/`-V` bare output (Task 6/7); server-aware status with graceful degradation (Task 4); `--json` (Task 5/6); sparkline art reusing the TUI palette (Task 2/5); Tome-parity exit codes (Task 4/6); classification table (Task 4); JSON shape (Task 5); tests incl. wiremock + snapshot-style assertions (Task 4/5/7). All spec sections map to a task.
- **Type consistency:** `assemble_report(Result<ClientConfig, ClientError>)`, `Overall::exit_code()`, `StatusReport { gauge, client, server, data, overall }`, `human_panel(&StatusReport) -> Vec<String>`, `emit(&StatusReport, bool)`, `art::sparkline(&[Color]) -> Vec<String>`, `ApiClient::{from_config_with_timeout, healthz, readyz}` are used identically across tasks.
- **No network in unit tests:** lib unit tests only exercise pure functions + the `Err` arm of `assemble_report`; all network paths are covered by `tests/status.rs` via wiremock.
- **`dead_code`:** every new item is `pub` (incl. `pub mod art;`), so incremental commits stay clean under `clippy -D warnings`.
