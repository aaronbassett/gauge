# Porting apps onto `gauge-telemetry`

This crate replaces an app's bespoke telemetry with a shared, privacy-first
kernel that sends to the Gauge backend. **Each migration is its own project
cycle**; this document is the plan input. API names below reflect the crate as
built.

## The shape of an integration

Build one `Telemetry` handle at startup, emit typed events on the hot path, and
flush out of band.

```rust
use gauge_telemetry::{Telemetry, Flusher};
use gauge_telemetry::common::{CommandInvoked, Outcome, Surface};

let telemetry = Telemetry::builder()
    .app("tome")                                  // = service.name; must be on the server allowlist
    .app_version(env!("CARGO_PKG_VERSION"))
    .endpoint("https://…")                        // https or loopback only — validated at build()
    .install_id_path(install_id_path)             // e.g. ~/.tome/telemetry/id
    .app_env_var("TOME_TELEMETRY")                // app's own opt-out var
    .config_enabled(cfg.telemetry_enabled)        // from app config
    .runtime_enabled(!runtime_off)                // from a runtime toggle
    .accel(Some("metal".into()))                  // optional, app-supplied
    .flush_args(vec!["telemetry".into(), "flush".into(), "--quiet".into()]) // for detached flush
    .build()?;                                     // disabled consent => cheap no-op handle

telemetry.emit(&CommandInvoked {
    command: "search".into(), duration_ms: 142, outcome: Outcome::Ok, surface: Surface::Cli,
});
```

## Common steps (any app)

1. **Add the dependency** (published on crates.io).
2. **Build the handle** via `Telemetry::builder()` as above. A disabled handle
   (consent off) is a true no-op and does zero filesystem work, so call sites
   can stay unconditional.
3. **Map events.** Use the common events where they fit
   (`common::{Install, Heartbeat, CommandInvoked, ToolCall, ErrorEvent}` with
   the shared `Outcome`/`Surface` enums). Define app-specific events as your own
   `#[derive(Serialize)]` struct implementing `Event` with a bare `name()`:
   ```rust
   #[derive(serde::Serialize)]
   struct Search { latency_ms: u32, candidates: u32, reranker_used: bool }
   impl gauge_telemetry::event::Event for Search {
       fn name(&self) -> std::borrow::Cow<'_, str> { "search".into() }
   }
   ```
   The client prefixes the bare name with `<app>.` (so `search` → `tome.search`).
   Bare names must be a single non-empty segment (no `.`, no surrounding
   whitespace, ≤128 bytes).
4. **Replace client-side buckets with raw integers.** All quantities
   (`duration_ms`, `latency_ms`, `cpu_cores`, `ram_gb`, counts, rank) ship as raw
   integers; bucketing happens at read time on the server
   ([gauge#22](https://github.com/aaronbassett/gauge/issues/22)). Attribute
   values must be scalars (string/bool/int/double); `None` fields must use
   `#[serde(skip_serializing_if = "Option::is_none")]`; `null`/non-finite floats
   are rejected at emit. Integer fields must fit `i64`/`u64`.
5. **Choose a flush trigger.**
   - **Short-lived CLI:** wire a hidden flush subcommand to `telemetry.run_flush()`
     (early in `main`, before any normal startup path), and call
     `telemetry.spawn_detached_flush()` at exit. The kernel re-execs the binary
     with `flush_args` as a detached child; a `GAUGE_TELEMETRY_FLUSH_CHILD`
     marker prevents recursion. Simpler adopters can instead call
     `telemetry.flush_blocking(gauge_telemetry::client::DEFAULT_FLUSH_TIMEOUT)`.
   - **Long-running (MCP/cloud server):** `Flusher::start(&telemetry, interval,
     threshold_bytes)` at boot; drop it at shutdown. `threshold_bytes == 0`
     disables the size trigger (interval-only).
6. **Wire consent controls.** `telemetry.reset()` regenerates the install UUID
   and clears the queue (wire to a `reset` subcommand); the runtime toggle maps
   to `.runtime_enabled(false)`. The global `GAUGE_TELEMETRY_DISABLE=1` kill
   switch disables telemetry for *any* app (it can only disable, never enable).
   On-by-default-with-notice; a 10-minute first-run grace (`DEFAULT_GRACE`) holds
   the first flush; CI is auto-disabled.
7. **Add a canary test.** Use `gauge_telemetry::canary::assert_no_forbidden` over
   every event type, extending `FORBIDDEN_SUBSTRINGS` with app-specific markers.
   Pay special attention to any free-form `String` field (e.g. `accel`, model
   ids): the scalar validator passes them by construction, so the canary is the
   only backstop — constrain them to a closed vocabulary or probe them
   explicitly.
8. **Allowlist the app.** Ensure `<service.name>` is on the server
   `GAUGE_APP_ALLOWLIST`.

## tome

- Wire format JSON → OTLP (handled by the kernel).
- Envelope remap is automatic: `os=macos→darwin`, `arch=x86_64→amd64`,
  `aarch64→arm64` (tome's `os=macos/linux`, `arch=x86_64/aarch64` strings
  disappear).
- All `count` / `rank` / `load` / `latency` buckets → raw integers.
- The `catalog.<id>.*` attributed stream must be renamed under the `tome.`
  namespace (e.g. `tome.catalog_*`) to satisfy the `<app>.` prefix rule; the
  hardcoded source allowlist + canonicalization logic stays in tome and feeds
  custom `Event` types.
- 18 anonymous + 6 catalog-attributed events → map to common events + custom
  events.
- `schema_version` / `sample_rate` / `calling_harness` → carry as optional
  attributes on the relevant events (no global envelope field in this kernel).
- tome already mints a persistent install UUID and runs a detached flusher, so
  `install_id_path` + the `flush_args`/`run_flush` wiring map directly.
- Retire `src/telemetry/` (its disk-queue is the model the kernel reuses).
- The `TELEMETRY.md` pin test becomes a canary suite + the crate's `SPEC.md`
  pin.

## midnight-manual

- Wire format JSON-array → OTLP (handled by the kernel).
- In-memory buffer + jittered backoff → the kernel's disk queue + `Flusher`
  (drop the backoff; at-least-once comes from the queue).
- Server-side CHECK-constraint schema → kernel scalar validation + a canary
  test.
- **Decision to confirm:** retire mn's own `/v1/telemetry/events` server path in
  favour of Gauge `/v1/logs` (and remove the server-side telemetry validator /
  `telemetry_schema_invalid` once the client no longer targets it).
- mn does not persist an install UUID today — the kernel now mints/persists one
  at `install_id_path`.
- Map the 7 event types (`mcp_tool_call`, `rerank`, `cli_command`,
  `ingest_complete`, `pull_models`, `mcp_startup`, `mcp_shutdown`) to common
  events (`ToolCall`, `CommandInvoked`, lifecycle) + custom events. `mcp_tool_call`'s
  raw `latency_ms: u32` already matches the raw-integer model.
- mn's three-mechanism opt-out (env var, config flag, runtime toggle) maps onto
  `.app_env_var(..)` / `.config_enabled(..)` / `.runtime_enabled(..)`; its
  opt-out marker file maps to the runtime/config inputs.
- Add `midnight-manual` to the Gauge `GAUGE_APP_ALLOWLIST`.
