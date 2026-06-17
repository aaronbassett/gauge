# Gauge Telemetry Kernel (`gauge-telemetry`) — Design

- **Date:** 2026-06-17
- **Status:** Approved design; ready for implementation planning.
- **Scope of this cycle:** Design and build the `gauge-telemetry` client crate. Porting
  tome and midnight-manual, and the server-side read-time numeric bucketing, are
  **separate cycles** (see [Out of scope](#out-of-scope--dependencies)).

---

## 1. Context & problem

Gauge is a privacy-first telemetry platform: apps send anonymous events as OTLP
logs to `POST /v1/logs`; the server stores them in Postgres (JSONB) and answers
aggregate questions via a query DSL. Two facts shaped this design:

- **The Gauge server has no predefined event-type registry.** `validate_batch()`
  (`crates/gauge-events/src/profile.rs`) enforces only *structural* rules: the
  `service.name` must be on the runtime app allowlist (`GAUGE_APP_ALLOWLIST`),
  each event name must be prefixed `<app>.`, attribute values must be scalars
  (string/bool/int/double), ≤30 attrs/record, strings ≤128 bytes, ≤1000
  records/batch. "Common" vs "custom" events is purely a **client-side**
  ergonomics distinction — the server accepts any well-formed event.
- **A low-level sender already exists** in `gauge-events` (the `sender` feature):
  `SenderConfig` + `enqueue()` (append to a crash-safe JSONL disk queue, no
  network on the hot path) + `drain()` (batch → POST → atomic rewrite,
  at-least-once). It is generic ("send any event name + scalar attrs") and lacks
  ergonomics for common events and typed custom events.

Meanwhile **both target apps already have mature, bespoke telemetry**:

- **tome** — `src/telemetry/` (18 modules): typed closed-record events (18
  anonymous + 6 catalog-attributed), **client-side bucketing**, opt-out, a
  catalog-attribution second stream, and a `TELEMETRY.md` byte-pinned by tests.
  It does **not** use `gauge-events` today and does **not** speak Gauge's OTLP
  wire format; its envelope uses `os=macos/linux`, `arch=x86_64/aarch64`.
- **midnight-manual** — `crates/mn-telemetry`: a closed event set (7 types)
  enforced by a server-side CHECK constraint, posting JSON arrays to its **own**
  endpoint `/v1/telemetry/events` (not Gauge), with an in-memory buffer +
  jittered exponential backoff and a three-mechanism opt-out resolver. It sends
  **raw `u32` latencies/durations** (no bucketing).

**Goal:** a single, publishable, "fat kernel" crate that both apps (and future
internal apps) adopt to send telemetry to the unified Gauge backend, replacing
their bespoke implementations. This is a **consolidation**, not greenfield.

## 2. Goals / non-goals

**Goals**
- A new library crate `gauge-telemetry` (the fat kernel) that owns events,
  identity, consent, environment capture, the disk queue, and the flush
  lifecycle.
- Serde-typed events with ready-made **common events** plus a frictionless
  **custom event** path.
- All quantities sent as **raw bounded integers**; bucketing deferred to read
  time.
- A concise set of coarse **environment attributes**.
- A **`PORTING.md`** detailed enough to plan the tome and mn migrations.

**Non-goals (separate cycles)**
- Porting tome (own spec/plan/implementation).
- Porting midnight-manual (own spec/plan/implementation).
- Server-side read-time numeric bucketing — tracked as
  [aaronbassett/gauge#22](https://github.com/aaronbassett/gauge/issues/22).

## 3. Decisions (locked)

| # | Decision | Rationale |
|---|---|---|
| D1 | The crate **owns everything** (events, identity, consent, env capture, queue, flush) — a "fat telemetry kernel". | Maximum sharing and a single audited privacy path; both apps' bespoke versions are replaced. |
| D2 | **This cycle = the crate only**; migrations + server bucketing are separate. | Smallest reviewable unit; prove the crate before touching audited code. |
| D3 | Quantities ride as **raw bounded integers**, bucketed **at read time**. | Emit-time bucketing irreversibly destroys information and freezes bucket edges into shipped binaries. Applies to latency, durations, CPU cores, RAM, counts, rank. |
| D4 | Events are authored **Approach A: serde-typed** (`T: Serialize` + a bare `name()`); scalar-only validated at emit + canary tests as the privacy backstop. | The pattern both apps already use → lowest porting risk, no proc-macro, fast compiles, legible errors. The crate is internal/source-available, so a compile-time guarantee (Approach B) isn't worth its cost. |
| D5 | Consent is **opt-out, on-by-default-with-notice**; `reset()` only (**no `purge()`**); **10-minute** first-run grace. | Matches both apps; 10 min is enough for a user to react to the notice — unattended runs are ~always CI, which is disabled anyway. |
| D6 | A global **`GAUGE_TELEMETRY_DISABLE=1`** kill switch overrides app opt-ins; `0`/unset defers to the app (disable-only, never force-enable). | One env var disables telemetry for *any* app using the kernel; the global var can only ever narrow collection, never widen it. |
| D7 | One **disk-queue** delivery substrate everywhere — **no in-memory backend** (mn standardizes onto it). | Single crash-safe, bounded, at-least-once path; uniform behaviour and one thing to test. |
| D8 | Two flush triggers: **detached at-exit flush** (hidden-subcommand contract) **and** background flush; plus a **`flush_blocking(timeout)`** zero-wiring fallback. | Detached flush gives CLIs true non-blocking exit; background flush serves long-running processes; the blocking fallback lets a simple adopter skip the subcommand wiring. |
| D9 | Environment attributes are a small, coarse set on a **low-frequency lifecycle event**, not on every event and not as resource attrs. | Joint entropy is the fingerprinting risk; the server only persists the six profile resource attrs + per-event attributes anyway. |

## 4. Architecture

```
gauge-events  (existing)  ──  OTLP wire types · profile validation · low-level disk-queue sender
      ▲
      │ depends on (reuses enqueue/drain/encode/transport/queue)
gauge-telemetry  (new — the "fat kernel")
      ├─ events:    Event trait + common event types + app-defined custom events
      ├─ identity:  install UUID (persisted) + session UUID (ephemeral)
      ├─ consent:   layered opt-out resolver (D5, D6)
      ├─ env:       coarse environment capture (D9)
      ├─ envelope:  builds the six Gauge-profile resource attributes (os/arch remap)
      └─ lifecycle: emit (sync append) · flush (detached / background / blocking)
```

`gauge-telemetry` depends on `gauge-events` for the OTLP types, profile
validation, and the low-level sender — it does **not** reinvent the queue or
transport. It is **published to crates.io** so the two separate repositories can
consume it. The Gauge **server** keeps depending only on `gauge-events`
(`validate_batch`), never on `gauge-telemetry`, so client-only concerns
(identity, consent, process-spawning) never reach the server build.

## 5. Public API (Approach A)

An event is any `Serialize` type that flattens to scalar attributes, plus a bare
name the client namespaces with `<app>.`:

```rust
pub trait Event: serde::Serialize {
    /// Bare event name, no app prefix. The client prepends "<app>." → "tome.search".
    fn name(&self) -> std::borrow::Cow<'_, str>;
}
```

Common events ship in the crate (the "shortcuts"); custom events are the app's
own serde structs:

```rust
// Common:
telemetry.emit(CommandInvoked {
    command: "search".into(), duration_ms: 142, outcome: Outcome::Ok, surface: Surface::Cli,
});

// Custom (the apps' current style):
#[derive(Serialize)]
struct Search { latency_ms: u32, candidates: u32, reranker_used: bool }
impl Event for Search { fn name(&self) -> Cow<'_, str> { "search".into() } }
telemetry.emit(Search { latency_ms: 142, candidates: 12, reranker_used: true });
```

The handle is built once at startup; a disabled handle is a cheap no-op so call
sites stay ergonomic:

```rust
let telemetry = Telemetry::builder()
    .app("tome").app_version(env!("CARGO_PKG_VERSION"))
    .endpoint("https://…fly.dev")
    .install_id_path(home.join(".tome/telemetry/id"))
    .consent(resolver)
    .build()?;
```

### Proposed common-event set

The genuinely cross-app events become first-class types; everything app-specific
stays a custom event. This set is the **proposed starting point implemented in
this cycle**; the porting cycles may extend it **additively** as tome's 18+6 and
mn's 7 catalogues are fully mapped (it is not frozen):

- **Lifecycle:** `install`, `upgrade`, `heartbeat`, `session_start`, `shutdown`,
  `cold_start`, `model_download`.
- **Activity:** `command_invoked` (command enum, `duration_ms`, `outcome`,
  `surface`, optional harness), `tool_call` (tool enum, `latency_ms`,
  `result_count`, `outcome`).
- **Failure:** `error` (`error_class` enum, `surface`, optional harness).

The environment attributes (§8) ride on `install` and `heartbeat`.

## 6. Identity

- **Install UUID** — random v4, persisted at the app-configured path (e.g.
  `~/.tome/telemetry/id`), mode `0600`, created race-safely with
  `O_CREAT|O_EXCL` on first run and reused after. Encodes nothing about the
  machine. It is `service.instance.id` on the wire. The file's **mtime = mint
  time**, used only for the first-run grace period (§7).
- **Session UUID** — random v4, minted in memory per process, never persisted.
  It is `session.id`.
- **`reset()`** regenerates the install UUID (severing future continuity) and
  clears the queue. (No `purge()` — D5.) The app wires this to its CLI.

## 7. Consent / opt-out

Opt-out model (on-by-default with a one-time notice), resolved once at
`build()`. **Disabled wins**, in this precedence:

```
1. GAUGE_TELEMETRY_DISABLE=1   → force disabled (global kill switch; beats app opt-ins)
   (=0 or unset → no signal; defer to the app)
2. runtime toggle off          → disabled
3. app env var (e.g. TOME_TELEMETRY=0) → disabled
4. app config flag off         → disabled
5. CI detected                 → disabled
   else → enabled, but the FIRST run holds a 10-minute grace (from install-id mtime)
          before the first flush, so a user who just saw the notice can opt out first.
```

The global var is **disable-only**: `GAUGE_TELEMETRY_DISABLE=0` never
force-enables, it only defers to the app — so it can only ever narrow collection.
The app supplies the specifics: its env-var **name**, the config bool, the
opt-out marker path, and the notice text. A disabled `build()` returns a cheap
no-op handle.

## 8. Resource envelope & environment attributes

The kernel always populates the six Gauge-profile **resource attributes** from
config + auto-detection, including the remaps that previously bit tome:

| Attribute | Source |
|---|---|
| `service.name` | config `app` |
| `service.version` | config `app_version` |
| `service.instance.id` | install UUID |
| `session.id` | session UUID |
| `os.type` | `std::env::consts::OS` → `macos→darwin`, `linux→linux`, `windows→windows` |
| `host.arch` | `std::env::consts::ARCH` → `x86_64→amd64`, `aarch64→arm64` |

**Environment attributes** (locked set), sent as scalar attributes on the
low-frequency `install`/`heartbeat` events — never on every event, never as
resource attrs (the server persists only the six resource attrs + per-event
attributes):

| Attribute | Wire form |
|---|---|
| OS version | `darwin:14` · `windows:11` · `ubuntu:22` · `arch` (distro + **major** only; **never** the kernel version string) |
| CPU cores | raw int, clamped to `u16` (read-time bucketed) |
| RAM | whole GB, rounded raw int (read-time bucketed) |
| Acceleration | enum `metal / cuda / rocm / cpu` (capability, not GPU model) |
| libc (Linux) | `glibc / musl` |
| Language | language subtag only, e.g. `en` (no region) |
| Shell | enum `bash / zsh / fish / pwsh / cmd / other` |

`in_container` was considered and **dropped**: its main use (denoising ephemeral
installs) is largely covered by CI exclusion and is better measured directly via
install-UUID churn; it failed the "will we act on it?" bar.

**Guiding principle:** the risk is *joint* entropy, not any single attribute.
Keep the set small, coarsen hard, prefer enums/bools over numbers.

## 9. Quantities & the read-time bucketing rule

> **All quantities** (latency, durations, CPU cores, RAM, counts, rank/position)
> ride as **raw bounded/rounded integers**; **all bucketing happens at read time**
> in the query layer. Client-side **closed enums** are reserved for genuine
> *categoricals* (`os.type`, `accel`, `libc`, `outcome`, `surface`, `language`,
> `shell`) — never for quantities.

This is the faithful realization of "don't destroy information at emit time" in a
logs-only system. It depends on the server gaining read-time numeric bucketing
([#22](https://github.com/aaronbassett/gauge/issues/22)); until then raw values
are stored but not yet histogrammable.

## 10. Delivery model

- **Hot path (`emit`)** — serialize → flatten one level → validate (scalar-only +
  limits) → **append one JSON line** to the disk queue. No network, no blocking,
  no runtime required. Reuses `gauge-events::enqueue`.
- **Queue substrate** — the existing `gauge-events` JSONL queue: `0600`,
  append-only, ~1 MiB cap, ~4 KiB/line cap, atomic rewrite after delivery,
  non-blocking lock (a second flusher exits), **at-least-once** across crashes.
- **Flush triggers (app picks per process type):**
  1. **Detached at-exit flush** (short-lived CLIs): at exit the kernel spawns a
     *detached* process (new session, I/O → `/dev/null`) that runs one `drain()`
     and exits; the parent exits **without waiting**. **Contract:** the app
     exposes a hidden flush subcommand and routes it to `telemetry.run_flush()`
     early in `main`; the kernel re-execs `current_exe` with the app-registered
     flush args. (A library can't just background a thread — that would block
     exit or get killed.)
  2. **Background flush** (long-running: MCP server, cloud server): a `Flusher`
     started at boot that drains every N seconds or past a queue-size threshold,
     dropped at shutdown.
  - **`flush_blocking(timeout)`** — a zero-wiring fallback for adopters who don't
    want the subcommand; blocks exit up to the timeout, best-effort.
- **Sync core, async-friendly** — `drain()` is blocking (reqwest blocking, 5 s
  timeout, no redirects, batches ≤100). Async apps run it via `spawn_blocking` or
  a dedicated thread; the kernel offers a helper but never requires a runtime.
- **Failure handling** — non-2xx/network error → keep the batch, stop, retry next
  flush. Queue full → drop per caps (bounded, lossy). Unparseable line → dropped
  permanently. All silent; telemetry never inconveniences the user.

## 11. Privacy & validation

- **`emit()` is non-fatal.** On a validation violation: **drop the event** (never
  enqueue), `debug_assert!` in dev (loud at the author's keyboard), silent in
  release. Telemetry can never break or block the app.
- **Scalar-only** — every attribute value must be string/bool/int/double; nested
  objects/arrays are rejected. (A `String` field still *compiles* — Approach A's
  accepted trade-off — so canaries are the backstop.)
- **Limits**, client-side, matching the profile: ≤30 attrs/record, string values
  ≤128 bytes, plus bare→`<app>.` name prefixing.
- **Canary support** — the kernel ships (1) the runtime scalar/length validator,
  (2) a reusable **canary harness** apps point at their own event types with a
  forbidden-substring corpus (tome/mn keep their suites, rebuilt on the shared
  helper), and (3) canary tests for the kernel's own common events.

## 12. Testing

- **Unit:** validation; install-UUID race (`O_CREAT|O_EXCL`); queue
  caps/atomic-rewrite/lock; **consent precedence** (incl. `GAUGE_TELEMETRY_DISABLE`
  + 10-min grace); os/arch remap; env detection.
- **Profile conformance:** an encoded batch passes `gauge-events::validate_batch`
  (the pattern already in `encode.rs`).
- **Canary:** the kernel's own common events; the shared harness.
- **Spec-pin:** a `SPEC.md` with byte-pinned worked examples (à la
  `gauge-events/SPEC.md` and tome's `telemetry_md_pin.rs`) so the wire contract
  can't silently drift.

## 13. Porting docs (a deliverable)

A `PORTING.md` shipped with the crate, detailed enough to *plan* each migration:

**tome**
- `macos→darwin` / `x86_64→amd64`; JSON → OTLP wire format.
- Bucketed values → raw ints (count/rank/load buckets included).
- The `catalog.<id>.*` attributed stream is renamed under the `tome.` namespace
  (the allowlist + canonicalization logic stays in tome as custom events) to
  satisfy the `<app>.` prefix rule.
- 18 + 6 event catalogue → common-events + custom mapping.
- `schema_version` / `sample_rate` / `calling_harness` handling.
- Subcommand wiring (flush / reset / toggle); retire `src/telemetry/`.

**midnight-manual**
- JSON-array → OTLP wire format.
- In-memory buffer + backoff → disk queue + background flush.
- Server-side CHECK-constraint schema → kernel scalar validation.
- **Decision to confirm:** retire mn's own `/v1/telemetry/events` in favour of
  Gauge `/v1/logs`.
- Persist an install UUID (mn doesn't today).
- Catalogue mapping; opt-out wiring.
- Add `midnight-manual` to the Gauge `GAUGE_APP_ALLOWLIST`.

## 14. Out of scope / dependencies

1. **Server read-time numeric bucketing** —
   [aaronbassett/gauge#22](https://github.com/aaronbassett/gauge/issues/22). Hard
   dependency for the raw-int decisions (D3, §9) to pay off.
2. **Port tome** — its own spec/plan/implementation cycle.
3. **Port midnight-manual** — its own spec/plan/implementation cycle.

## 15. Open questions / risks

- **Common-event taxonomy (§5)** ships as a proposed starting set this cycle and
  is extended **additively** during the porting cycles as the full tome/mn
  catalogues are mapped — the set is not frozen.
- **Detached-flush portability** — the re-exec/detach pattern is proven on Unix
  (tome); Windows behaviour must be verified during implementation.
- **`flush_blocking` timeout** default needs a concrete value chosen in planning.
- **crates.io publishing cadence** — the crate must be published before the app
  migrations can depend on it; coordinate with the existing release-plz setup.
