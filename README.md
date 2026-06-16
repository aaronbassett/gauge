# Gauge

**Privacy-first telemetry for developer tooling.** Gauge collects anonymous usage
events from CLI/MCP/desktop apps over standard OTLP, stores them in Postgres, and
answers questions about them — through an authenticated query API, a terminal
dashboard, and an MCP server you can point an agent at.

[![CI](https://github.com/aaronbassett/gauge/actions/workflows/ci.yml/badge.svg)](https://github.com/aaronbassett/gauge/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![Rust 2024](https://img.shields.io/badge/rust-2024%20edition%20%C2%B7%201.93%2B-orange.svg)](rust-toolchain.toml)
[![Status](https://img.shields.io/badge/status-pre--release%20v0.1.0-yellow.svg)](#project-status)

> **Status:** `v0.1.0`, pre-release. The platform is built, tested (114 passing),
> and verified end-to-end locally. The production Fly.io deploy is **deferred to a
> human** — see [Deployment](#deployment). Built for Midnight DevRel tooling
> (Tome, Midnight Manual); designed to take any OTLP-speaking app.

---

## Contents

- [Why Gauge](#why-gauge)
- [How it works](#how-it-works)
- [The workspace](#the-workspace)
- [Quickstart](#quickstart)
- [The event standard — the Gauge OTLP profile](#the-event-standard--the-gauge-otlp-profile)
- [The server (`gauge-server`)](#the-server-gauge-server)
- [The client (`gauge`)](#the-client-gauge)
- [Sending telemetry from your app](#sending-telemetry-from-your-app)
- [Privacy model](#privacy-model)
- [Deployment](#deployment)
- [Development](#development)
- [Project status](#project-status)
- [License](#license)

---

## Why Gauge

Most analytics stacks make you choose between *useful* and *respectful*. Gauge is
built so you never have to:

- **Anonymous by construction.** Ingestion needs no auth and stores no identity. IP
  addresses live only in the in-memory rate limiter; they are never written next to
  an event. There is no per-user drill-down anywhere in the system — only aggregate
  counts and counts of unique installs/sessions.
- **One open standard, many apps.** Events are plain [OTLP](https://opentelemetry.io/docs/specs/otlp/)
  logs-signal JSON. Any OTLP-conformant exporter can ship to Gauge; first-party apps
  use a tiny, dependency-light sender instead of the full OTel SDK. New apps onboard
  with **zero server code changes** — just an allowlist entry.
- **Questions, not dashboards-only.** A typed query DSL (Cube-style measures /
  dimensions / filters) powers a fixed-layout TUI *and* an MCP server, so a human and
  an agent ask the same questions of the same data. "What's our most-used command?"
  "How many unique users did X this week?" — both answerable in one call.
- **Boring, auditable Rust.** A single Cargo workspace, compile-checked everywhere,
  `clippy -D warnings`, `cargo-deny`, and privacy *canary tests* that fail CI if an IP
  column or a leaked attribute value ever sneaks in.

## How it works

```
  Tome ─────┐
            │  OTLP/HTTP (JSON)                  ┌─────────────────────┐
  Midnight ─┼─── POST /v1/logs ──── anonymous ─► │     gauge-server    │     ┌──────────────┐
  Manual    │    (rate-limited per IP,           │   axum on Fly.io    │ ──► │ Fly Managed  │
            │     IP never stored)               │                     │     │  Postgres    │
  future ───┘                                    │  /v1/auth/challenge │     │  (JSONB      │
  apps                                           │  /v1/auth/verify    │     │   events)    │
                                                 │  /v1/query          │     └──────────────┘
  gauge client ── Ed25519 challenge/response ──► │  /v1/meta           │
  (TUI + MCP)  ── Bearer JWT ── POST /v1/query ─►└─────────────────────┘
```

Two binaries, three shared crates. Apps **send** anonymous events; operators
**read** them with an Ed25519-authenticated client.

## The workspace

A five-crate Cargo workspace (edition 2024, resolver 3, Rust 1.93):

| Crate | Kind | Responsibility |
|---|---|---|
| [`gauge-events`](crates/gauge-events) | lib | The Gauge OTLP profile — serde wire types, validation, and (`sender` feature) a crash-safe batching client. Ships the canonical [`SPEC.md`](crates/gauge-events/SPEC.md). |
| [`gauge-query`](crates/gauge-query) | lib | The query language — request/response types (serde + schemars) shared by server, TUI, and MCP so the surfaces can't drift. |
| [`gauge-auth`](crates/gauge-auth) | lib | The auth protocol — Ed25519 keypairs, `ed25519:<base64>` wire format, single-use challenges, HS256 JWTs. |
| [`gauge-server`](crates/gauge-server) | bin | axum service: OTLP ingest, auth, the DSL→SQL query engine, `/v1/meta`, rate limiting, privacy canaries. sqlx + Postgres. |
| [`gauge`](crates/gauge) | bin | The reader: `keys` · `login` · `query` · `tui` · `mcp serve`. |

`gauge-events`, `gauge-query`, and `gauge-auth` are consumed via path today; they're
designed to publish to crates.io once the schema settles.

## Quickstart

You need a recent stable Rust (the toolchain is pinned to **1.93** in
[`rust-toolchain.toml`](rust-toolchain.toml)) and **Postgres** to run the server.
The workspace builds offline — sqlx uses runtime queries, so no database is required
at compile time.

```bash
git clone git@github.com:aaronbassett/gauge.git
cd gauge
cargo build --release   # builds both binaries; no DB needed
```

### 1. Run the server locally (with demo data)

`ENABLE_DEMO_MODE=1` exposes an unauthenticated `POST /v1/mock` that generates
realistic, profile-shaped synthetic events — the fastest way to see Gauge with data.
**Never enable it in production.**

```bash
# A throwaway local Postgres (migrations run automatically at boot):
docker run -d --name gauge-pg -e POSTGRES_PASSWORD=pw -p 5432:5432 postgres:16

export DATABASE_URL="postgres://postgres:pw@localhost:5432/postgres"
export GAUGE_JWT_SECRET="$(openssl rand -base64 48)"     # ≥ 32 bytes
export GAUGE_APP_ALLOWLIST="tome,midnight-manual"
export GAUGE_USER_STORE="$(cat users.toml)"              # see step 2
export ENABLE_DEMO_MODE=1

cargo run --release -p gauge-server
# → listening on 0.0.0.0:8080, migrations applied

# In another shell — seed ~500 synthetic events over the last 30 days:
curl -s -X POST localhost:8080/v1/mock -d '{"count": 500}'
```

### 2. Register a reader and log in

Readers authenticate with an Ed25519 key. Generate one, drop its public half into
the server's user store, then log in.

```bash
# Generate a keypair (private seed → ~/.config/gauge/<id>.private, mode 0600):
gauge keys generate --user-id aaron
# prints a ready-to-paste [[users]] block:
#   [[users]]
#   user_id = "aaron"
#   role = "viewer"
#   public_key = "ed25519:…"
```

Put that block in a `users.toml` and feed its **contents** to the server via
`GAUGE_USER_STORE` (the value is the file *content*, not a path):

```toml
schema_version = 1

[[users]]
user_id = "aaron"
role = "admin"
public_key = "ed25519:<from the command above>"
created_at = "2026-06-13"
```

Tell the client where the server is (`~/.config/gauge/config.toml`):

```toml
server_url = "http://localhost:8080"
user_id    = "aaron"
```

```bash
gauge login        # Ed25519 challenge/response → cached JWT (1h, auto-refreshed)
```

### 3. Ask questions

```bash
# One-shot DSL query (JSON in, JSON out):
gauge query '{"measures":["unique_installs"],"dimensions":["event_name"],
              "time_range":{"last":"7d"},"order":[{"field":"unique_installs","dir":"desc"}],
              "limit":10}'

gauge tui          # the dashboard (q quit · tab switch page · t cycle range · r refresh)

gauge mcp serve    # MCP server over stdio — wire it into an agent
```

## The event standard — the Gauge OTLP profile

Gauge ingests **standard OTLP/HTTP, logs signal, JSON encoding** at `POST /v1/logs`.
On top of plain OTLP it requires a thin profile. The canonical definition — with
byte-for-byte worked examples pinned by tests — lives in
[`crates/gauge-events/SPEC.md`](crates/gauge-events/SPEC.md). The essentials:

**Required resource attributes** (one resource block per batch — all standard OTel
semantic conventions):

| Attribute | Meaning | Constraint |
|---|---|---|
| `service.name` | App id (`tome`, `midnight-manual`, …) | Must be on the server allowlist |
| `service.version` | Sender semver | Non-empty |
| `service.instance.id` | **Anonymous install UUID** — the uniqueness join key | RFC-4122 v4, random per install, user-resettable |
| `session.id` | Per-process session UUID | RFC-4122 v4 |
| `os.type` | Platform | `darwin` · `linux` · `windows` |
| `host.arch` | CPU architecture | `amd64` · `arm64` |

**Events** are one `LogRecord` each. The event name comes from the `eventName` field
(OTLP ≥ 1.4) or an `event.name` attribute — the sender writes both. Names are
namespaced per app: `<service.name>.<event>` (e.g. `tome.search`). Attributes are
flat key/value pairs (string/bool/int/double).

**Hygiene limits** (per-record violations → OTLP *partial success*; bad envelope →
whole-batch 400):

| Limit | Value |
|---|---|
| Attributes per record | ≤ 30 |
| Attribute string value length | ≤ 128 bytes |
| Records per batch | ≤ 1,000 |
| Request body size | ≤ 1 MiB |

## The server (`gauge-server`)

axum + tower, sqlx + Postgres, tokio, JSON tracing with request IDs. Migrations run
at boot. A single `events` table stores typed envelope columns plus a JSONB
`attributes` blob; `install_id` and `session_id` are columns the query layer can
*count* but never *expose*.

### Endpoints

| Endpoint | Auth | Purpose |
|---|---|---|
| `POST /v1/logs` | none | OTLP ingest; validates the profile; batch insert; OTLP success / partial-success response |
| `POST /v1/auth/challenge` | none | `{user_id}` → `{challenge_id, nonce_b64, expires_in_s}` (single-use, 60s TTL) |
| `POST /v1/auth/verify` | none | `{challenge_id, signature_b64}` → `{token, user_id, expires_at}` (HS256 JWT, 1h) |
| `POST /v1/query` | Bearer JWT | Query DSL → aggregated rows |
| `GET /v1/meta` | Bearer JWT | Known apps, event names, attribute keys, earliest/latest event time |
| `GET /healthz`, `GET /readyz` | none | Liveness / readiness (DB ping) |
| `POST /v1/mock` | none | **Demo only** — synthetic data generator; 404 unless `ENABLE_DEMO_MODE=1` |

### Query DSL

A query request compiles to **one parameterized SQL statement** (identifiers come
from closed enums; values are always bound), run in a read-only transaction with a 5s
statement timeout.

- **Measures:** `count`, `unique_installs` (`COUNT(DISTINCT install_id)`), `unique_sessions`
- **Dimensions:** `app`, `event_name`, `app_version`, `os`, `arch`, `attr.<key>`, plus a time bucket when `granularity` is set
- **Filters:** `eq` · `neq` · `in` · `exists` over those fields
- **Time range:** relative (`{"last":"7d"}`) or absolute (`{"from":…,"to":…}` RFC3339)
- **Granularity:** `hour` · `day` · `week`
- **Order / limit:** order by any selected field; limit defaults to 1,000, hard cap 10,000; the response flags `truncated`

```jsonc
// POST /v1/query
{
  "measures": ["unique_installs"],
  "dimensions": ["app", "event_name"],
  "filters": [{ "field": "app", "op": "eq", "value": "tome" }],
  "time_range": { "last": "7d" },
  "granularity": "day",
  "order": [{ "field": "unique_installs", "dir": "desc" }],
  "limit": 100
}
// →
{
  "rows": [
    { "time": "2026-06-11T00:00:00Z", "app": "tome", "event_name": "tome.search", "unique_installs": 42 }
  ],
  "truncated": false,
  "elapsed_ms": 12
}
```

### Authentication

Reimplements the Midnight Manual admin pattern: Ed25519 **challenge/response → JWT**.
Public keys are wire-encoded `ed25519:<base64>`. The user store is TOML
(`schema_version = 1`), supplied as the *content* of the `GAUGE_USER_STORE` secret,
loaded once at boot and immutable at runtime — **adding a reader = edit the secret +
restart**. Challenges are 32-byte nonces, single-use, 60s TTL; unknown users 404
(no enumeration). JWTs are HS256, 1h TTL, with the signing secret wrapped in an
opaque type so it can't leak via `Debug`. A `role` field (`admin` | `viewer`) exists
for forward compatibility; v1 treats all authenticated readers identically.

### Configuration

| Env var | Required | Default | Purpose |
|---|:---:|---|---|
| `DATABASE_URL` | ✅ | — | Postgres connection string |
| `GAUGE_JWT_SECRET` | ✅ | — | HS256 secret, ≥ 32 bytes |
| `GAUGE_USER_STORE` | ✅ | — | `users.toml` **content** (the secret value, not a path) |
| `GAUGE_APP_ALLOWLIST` | ✅ | — | Comma-separated allowed `service.name` values |
| `GAUGE_LISTEN_ADDR` | | `0.0.0.0:8080` | Bind address |
| `GAUGE_RATE_LOGS_PER_MIN` | | `60` | Per-IP ingest rate limit |
| `GAUGE_RATE_AUTH_PER_MIN` | | `10` | Per-IP auth rate limit |
| `GAUGE_RATE_USER_PER_MIN` | | `120` | Per-user query/meta rate limit |
| `ENABLE_DEMO_MODE` | | _off_ | `=1` mounts the unauthenticated `/v1/mock` generator |

Rate-limited requests get `429` + `Retry-After`.

## The client (`gauge`)

One binary, five subcommands. Config lives at `~/.config/gauge/config.toml`
(`$XDG_CONFIG_HOME` and `GAUGE_CONFIG_DIR` are respected); the private key seed
(`<user_id>.private`, mode 0600) and cached token (`token.json`) sit beside it.

```
gauge keys generate --user-id <id>   # Ed25519 keypair; prints the [[users]] block to register
gauge login                          # challenge/response → cached JWT
gauge query '<json>'                  # one-shot DSL query (scripting / debugging)
gauge tui                            # dashboard
gauge mcp serve                      # MCP server over stdio
```

The internal API layer injects the Bearer token and, on a `401`, **transparently
re-runs the login handshake with the local key and retries once** — so long TUI
sessions and agent conversations never break on the 1-hour token expiry.

### TUI

Fixed pages, in the `btm`/`sampler` idiom:

1. **Overview** — events-over-time braille chart, big-number tiles (events today;
   unique installs 24h/7d/30d), top event types, an apps summary table.
2. **App detail** (one page per app from `/v1/meta`) — event-type breakdown, version
   distribution, os/arch split, per-event sparklines.
3. **Explore** — an interactive query builder over the DSL.

Rendering is pure and decoupled from polling (default 30s; `r` forces a refresh), so
a slow network never blocks the UI — failures degrade to a stale-data banner.
Keys: `q` quit · `tab` switch page · `t` cycle time range · arrows navigate.

### MCP server

`gauge mcp serve` exposes the query surface to an agent over stdio. Tool schemas are
generated from the shared `gauge-query` types, so the MCP tools can't drift from the
REST API.

| Tool | Purpose |
|---|---|
| `query_telemetry` | The full DSL — arbitrary questions |
| `get_meta` | Discovery: what apps / events / attribute keys exist |
| `unique_users` | Distinct installs over a period (+ optional app/event filters) |
| `top_events` | "Most used X": top-N event types by count or uniques |
| `events_over_time` | Timeseries for trend questions |

The three convenience tools are thin wrappers over the DSL — agents answer
"how many unique users did Y this week?" more reliably from a purpose-named tool than
by composing a query object. When unauthenticated, tools return an MCP error with
remediation: *run `gauge login`*.

## Sending telemetry from your app

First-party apps depend on `gauge-events` with the `sender` feature — no OTel SDK, no
protobuf. The sender is a crash-safe, at-least-once disk queue.

```toml
[dependencies]
gauge-events = { path = "../gauge/crates/gauge-events", features = ["sender"] }
```

1. **Build a `SenderConfig` once per process** — `app`, `app_version`, a persistent
   per-install `install_id` UUID, a per-run `session_id` UUID, `os`/`arch`, and a
   local `queue_path`.
2. **`enqueue(&cfg, "app.event_name", attrs)`** on each event. Cheap and crash-safe;
   never blocks your app. Event names **must** be prefixed `"{app}."`; attributes are
   bounded (≤ 30/record, ≤ 128 bytes/string). Send bucketed counts and closed enums —
   never query text, paths, hostnames, usernames, emails, or free-form strings.
3. **`drain(&cfg)`** on a timer / at shutdown. POSTs in ≤ 100-event batches, removes
   lines only after a `2xx` (at-least-once; a crash mid-drain re-sends), single-flighted
   via a lock file.

> **Gotcha:** `SenderConfig.endpoint` is the **base URL** (e.g.
> `https://gauge-telemetry.fly.dev`) — the transport appends `/v1/logs` itself.
> The endpoint must be `https://` (or loopback for local dev), so senders fail closed
> unless pointed at a real HTTPS endpoint. Always do a real-server smoke test
> (enqueue → drain → confirm via `gauge query`) when onboarding a new app: the unit
> tests use a mock that 200s anything and won't catch profile-validation mistakes.

**Server-side onboarding** is one line: add the app to `GAUGE_APP_ALLOWLIST` and
restart. Events from unknown apps are rejected whole.

## Privacy model

Privacy is a property the code *enforces*, not a promise in a doc:

- **No identity at ingest.** No auth to submit. No IP or User-Agent is ever persisted
  with an event — IPs exist only in the in-memory rate limiter. Ingest-path logs never
  contain bodies or attribute values; rejected-record errors never echo values.
- **No drill-down, by design.** `install_id` and `session_id` are storage columns the
  query layer can only *count* (`unique_installs`, `unique_sessions`) — they are
  deliberately **not** addressable as dimensions or filters. There is no endpoint,
  tool, or query that returns one user's activity.
- **Canary tests.** CI fails if the schema ever grows an IP/UA column, or if captured
  ingest tracing output ever contains an attribute value. These tests are non-vacuous
  and have never been weakened.
- **Duplicates tolerated.** Senders are at-least-once; analytics tolerate rare
  duplicates rather than carry dedup machinery.

## Deployment

Target: a [Fly.io](https://fly.io) app (working name `gauge-telemetry`) with Fly
Managed Postgres in `lhr`, forced HTTPS, `/healthz` + `/readyz` checks, migrations at
boot. The image is a multi-stage `cargo-chef` → distroless build (~60 MB).

**The live deploy is intentionally deferred to a human** — it provisions billable
infrastructure and a public endpoint, which shouldn't be an automated decision.
Deploy-readiness is fully verified locally (image builds; the release binary boots
against Postgres, runs migrations, serves `/healthz`/`/readyz` = 200, ingests the
SPEC worked example, and 401s unauthenticated queries).

The full runbook — `fly apps create`, Managed Postgres, secrets, `fly deploy`, and
post-deploy verification — is in [`docs/deploy.md`](docs/deploy.md).

## Development

```bash
cargo build                         # builds offline (runtime sqlx; no DB needed)
cargo test --workspace --all-features --locked   # 114 tests
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

CI ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) runs three jobs on every
push: **lint** (`fmt` + `clippy -D warnings`), **test** (full suite against a Postgres
service container), and **deny** (`cargo-deny` — advisories + licenses).

Design docs live under [`docs/superpowers/`](docs/superpowers): the
[design spec](docs/superpowers/specs/2026-06-12-gauge-telemetry-platform-design.md),
the [implementation plan](docs/superpowers/plans/2026-06-12-gauge-platform.md), and the
[implementation report](docs/superpowers/reports/2026-06-12-implementation-report.md).

## Project status

`v0.1.0`, pre-release. All 33 planned tasks are built, merged, and green; the platform
is verified end-to-end locally. Known next steps, explicitly out of v1 scope:

- **Sender migrations** — Tome and Midnight Manual adopt `gauge-events`.
- **Go live** — run the Fly.io deploy runbook.
- **Publish crates** — `gauge-events` (and likely `gauge-query`/`gauge-auth`) to crates.io once stable.
- **Scale levers** — GIN index on `attributes`, rollup tables, partitioning, retention — added only if query latency demands them.
- **Roles with teeth** — admin-only data-management endpoints (e.g. install-id purge for GDPR-style requests), token revocation via `jti`.

## License

Licensed under either of [MIT](https://opensource.org/licenses/MIT) or
[Apache-2.0](https://www.apache.org/licenses/LICENSE-2.0) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for
inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual-licensed as above, without any additional terms or conditions.
