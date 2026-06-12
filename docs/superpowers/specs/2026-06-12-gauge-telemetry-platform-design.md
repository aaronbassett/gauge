# Gauge — Telemetry Platform Design

- **Date:** 2026-06-12
- **Status:** Approved (brainstorming complete; awaiting implementation plan)
- **Repo:** `gauge` (new Cargo workspace)

## 1. Overview

Gauge is a telemetry platform for DevRel/Midnight tooling. It has two deliverables:

1. **`gauge-server`** — a Rust (axum) service on Fly.io with Fly Managed Postgres that is both the
   ingestion endpoint for anonymous telemetry from Tome, Midnight Manual, and future apps, and the
   authenticated API for reading the collected data.
2. **`gauge`** — a separate Rust client binary that renders the data as a TUI dashboard for humans
   (ratatui, in the style of sampler/btm/gtop) and serves it to agents as an MCP server
   (e.g. "What is our most used X?", "How many unique users did Y in the last week?").

Shared functionality lives in workspace crates used by both binaries (and, later, by sender apps).

### Goals

- One common event standard for all senders, extensible to future apps without server code changes.
- Anonymous ingestion (no auth to submit; no IP/identity ever stored with events).
- Authenticated read access using the Midnight Manual admin pattern (Ed25519 challenge/response → JWT).
- A query surface flexible enough for ad-hoc agent questions and TUI dashboards alike.

### Non-goals (this project)

- Migrating Tome and Midnight Manual to the new standard (separate follow-up projects, one per app;
  both apps are pre-first-release so the change is uncoordinated-breakage-free).
- Pre-aggregation/rollup pipelines (query raw events; revisit if latency ever hurts).
- Publishing shared crates to crates.io (consume via path/git until the schema settles).
- Configurable TUI dashboard layouts (fixed pages in v1).
- Write/admin API endpoints beyond ingestion (no data deletion/management API in v1).

## 2. Decisions log

| # | Decision | Choice | Alternatives considered |
|---|----------|--------|------------------------|
| 1 | Project scope | Gauge only; sender migrations are follow-ups | Include migrations; pilot one app |
| 2 | Event standard | OpenTelemetry — OTLP logs signal, events as LogRecords | Custom Tome-style envelope; schema-less envelope |
| 3 | OTLP depth | Wire format only (OTLP/HTTP **JSON**); lightweight shared crate, no OTel SDK/protobuf in senders | Full opentelemetry-rust SDK; dual JSON+protobuf parsing |
| 4 | Anonymous identity | Require `service.instance.id` = random v4 install UUID (Tome-style) as the uniqueness join key | No stable ID (status quo in Midnight Manual — makes unique-user queries impossible) |
| 5 | Auth code | Reimplement mn-auth's design as `gauge-auth` in this workspace | Extract mn-auth to a published crate; git dependency on midnight-manual |
| 6 | Query API | Typed JSON query DSL (Cube-style measures/dimensions/filters) over REST | GraphQL (poor fit for aggregations; heavy deps); fixed endpoints only; read-only SQL passthrough |
| 7 | Structure | Single Cargo workspace, JSONB event store, one client binary | Split repos + published crates; rollup tables now |

## 3. Architecture

```
  Tome ─────┐
            │  OTLP/HTTP (JSON)                  ┌─────────────────────┐
  Midnight ─┼─── POST /v1/logs ──── anonymous ─► │     gauge-server    │     ┌──────────────┐
  Manual    │    (rate-limited per IP,           │   axum on Fly.io    │ ──► │ Fly Managed  │
            │     IP never stored)               │                     │     │  Postgres    │
  future ───┘                                    │  /v1/auth/challenge │     └──────────────┘
  apps                                           │  /v1/auth/verify    │
                                                 │  /v1/query          │
  gauge client ── Ed25519 challenge/response ──► │  /v1/meta           │
  (TUI + MCP)  ── Bearer JWT ── POST /v1/query ─►└─────────────────────┘
```

### Workspace layout

```
gauge/
├── Cargo.toml                 # workspace
├── crates/
│   ├── gauge-events/          # OTLP wire types (serde), Gauge profile validation,
│   │                          # batching sender client, SPEC.md (the standard)
│   ├── gauge-query/           # query DSL request/response types (serde + schemars)
│   ├── gauge-auth/            # Ed25519 keypair/sign/verify, challenge + JWT types,
│   │                          # client-side login flow helper
│   ├── gauge-server/          # axum binary: ingest, query, auth, meta; sqlx/Postgres
│   └── gauge/                 # client binary: tui | mcp serve | keys | login | query
├── migrations/                # sqlx migrations (baked into server image)
├── docs/
└── fly.toml / Dockerfile.server
```

| Crate | Used by | One-line responsibility |
|---|---|---|
| `gauge-events` | server, client, future senders | Speak and validate the Gauge OTLP profile |
| `gauge-query` | server, client | Define the query language and result shapes |
| `gauge-auth` | server, client | Implement the challenge/response + JWT protocol |
| `gauge-server` | — | Persist events; answer queries; authenticate readers |
| `gauge` | — | Show humans dashboards; give agents query tools |

## 4. The event standard — "Gauge OTLP profile"

The standard is the **OTLP/HTTP logs-signal JSON encoding** plus the profile requirements below.
Because ingest is genuine OTLP, stock OpenTelemetry exporters/collectors can also ship to it;
our senders use the lightweight `gauge-events` client instead of the OTel SDK.

### Required resource attributes (one resource block per batch)

All are existing OTel semantic conventions:

| Attribute | Meaning | Constraint |
|---|---|---|
| `service.name` | App id (`tome`, `midnight-manual`, …) | Must match the server's app allowlist |
| `service.version` | Sender app semver | Non-empty string |
| `service.instance.id` | **Anonymous install UUID** — uniqueness join key | RFC-4122 v4, minted randomly per install, stored locally, user-resettable |
| `session.id` | Per-process session UUID | RFC-4122 v4 |
| `os.type` | Platform | Closed set: `darwin`, `linux`, `windows` |
| `host.arch` | CPU architecture | Closed set: `amd64`, `arm64` |

### Events

- Each event is one LogRecord. The event name is read from the LogRecord `eventName` field
  (OTLP ≥ 1.4 JSON encoding) or, as fallback, from an `event.name` attribute; the `gauge-events`
  client writes **both**. Records with neither are rejected.
- Event names are namespaced per app: `<service.name>.<event>` (e.g. `tome.search`,
  `midnight-manual.mcp_tool_call`). The prefix must equal the batch's `service.name`.
- Event attributes are flat key/value pairs (string/bool/int/double). Timestamps come from
  `timeUnixNano`; the server also records `received_at`.

### Server-enforced hygiene limits

| Limit | Value |
|---|---|
| Attributes per record | ≤ 30 |
| Attribute string value length | ≤ 128 bytes |
| Records per batch | ≤ 1,000 |
| Request body size | ≤ 1 MiB |

Violating records are rejected via OTLP **partial success** (rejected count + reason); a
violating batch envelope (bad resource attrs, unknown app) is rejected whole with 400.

### Privacy responsibilities

- **Server guarantees:** no IP address or User-Agent is ever persisted with events (IPs exist only
  in the in-memory rate limiter); ingest-path logs never contain bodies or attribute values;
  rejected-record errors never echo attribute values.
- **Sender obligations (documented in SPEC.md, enforced by sender-side tests at migration time):**
  bucketed counts and closed enums only; no query text, paths, hostnames, usernames, emails,
  free-form error messages, or any free-form string outside the registered shape. This follows
  Tome's TELEMETRY.md discipline, which both sender migrations will keep.
- Duplicate delivery is possible (senders are at-least-once); analytics tolerate rare duplicates
  and no dedup machinery is built.

`gauge-events` ships: serde types for the OTLP subset, profile validation (shared by server and
senders), a batching client (reqwest + rustls, 5s timeout, HTTPS-only, local disk queue with
crash-safe drain modeled on Tome's flush design), and `SPEC.md` whose worked examples are pinned
byte-for-byte by tests.

## 5. Server design (`gauge-server`)

**Stack:** axum + tower, sqlx (compile-time-checked queries, migrations at boot), PostgreSQL
(Fly Managed Postgres), tokio, tracing (JSON logs + request IDs). Deployment artifacts (multi-stage
cargo-chef → distroless Dockerfile, fly.toml, health checks) follow the proven midnight-manual
server templates.

### Endpoints

| Endpoint | Auth | Purpose |
|---|---|---|
| `POST /v1/logs` | none | OTLP ingest; validates profile; batch insert; OTLP success/partial-success response |
| `POST /v1/auth/challenge` | none | `{user_id}` → `{challenge_id, nonce_b64, expires_in_s}` (single-use, 60s TTL) |
| `POST /v1/auth/verify` | none | `{challenge_id, signature_b64}` → `{token, user_id, expires_at}` (HS256 JWT, 1h) |
| `POST /v1/query` | Bearer JWT | Query DSL → aggregated rows |
| `GET /v1/meta` | Bearer JWT | Known apps, event names, attribute keys, earliest/latest event time |
| `GET /healthz`, `GET /readyz` | none | Liveness / readiness (DB ping) for Fly checks |

### Storage

```sql
CREATE TABLE events (
    id           BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    app          TEXT        NOT NULL,
    app_version  TEXT        NOT NULL,
    install_id   UUID        NOT NULL,
    session_id   UUID        NOT NULL,
    os           TEXT        NOT NULL,
    arch         TEXT        NOT NULL,
    event_name   TEXT        NOT NULL,
    time         TIMESTAMPTZ NOT NULL,
    received_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    attributes   JSONB       NOT NULL DEFAULT '{}'::jsonb
);
CREATE INDEX events_app_name_time_idx    ON events (app, event_name, time);
CREATE INDEX events_app_install_time_idx ON events (app, install_id, time);
```

Envelope fields are typed columns; event attributes are JSONB. A GIN index on `attributes` is
deferred until attribute-filter query performance requires it. No partitioning in v1.

### Query engine

Translates a `gauge-query` request into **one parameterized SQL statement** (never string-spliced
values; identifiers from closed enums only). Executed in a read-only transaction with a 5s
statement timeout.

- **Measures:** `count`, `unique_installs` (`COUNT(DISTINCT install_id)`), `unique_sessions`
- **Dimensions:** `app`, `event_name`, `app_version`, `os`, `arch`, `attr.<key>`
  (`attributes->>'<key>'`), plus a time bucket when `granularity` is set (`date_trunc`)
- **Filters:** `eq` / `neq` / `in` / `exists` over the same fields
- **Time range:** relative (`{"last": "<N>h" | "<N>d"}`) or absolute (`{from, to}` RFC3339)
- **Granularity:** `hour` | `day` | `week`
- **Order/limit:** order by any selected measure/dimension; limit default 1,000, hard cap 10,000;
  response flags truncation

Example request and response:

```json
POST /v1/query
{
  "measures": ["unique_installs"],
  "dimensions": ["app", "event_name"],
  "filters": [{"field": "app", "op": "eq", "value": "tome"}],
  "time_range": {"last": "7d"},
  "granularity": "day",
  "order": [{"field": "unique_installs", "dir": "desc"}],
  "limit": 100
}

{
  "rows": [
    {"time": "2026-06-11T00:00:00Z", "app": "tome", "event_name": "tome.search", "unique_installs": 42}
  ],
  "truncated": false,
  "elapsed_ms": 12
}
```

### Authentication (mn-auth design, reimplemented)

- **Keys:** Ed25519; public keys in wire form `ed25519:<base64>` (32 bytes; padded and unpadded
  base64 both accepted on parse).
- **User store:** TOML (`schema_version = 1`; `[[users]]` with `user_id`, `role`, `public_key`,
  `created_at`, `note`), supplied as the *content* of the `GAUGE_USER_STORE` secret, loaded once
  at boot, immutable at runtime. Adding a reader = edit secret + restart.
- **Challenge:** 32-byte nonce, UUID challenge id, in-memory single-use store, TTL clamped to 60s,
  expired entries purged. Unknown `user_id` → 404 (no enumeration); consumed/unknown challenge →
  404; expired → 401; bad signature (64-byte Ed25519 over the raw nonce) → 403.
- **JWT:** HS256 with `GAUGE_JWT_SECRET` (≥ 32 bytes, wrapped in an opaque type to prevent Debug
  leaks). Claims: `sub` (user_id), `iat`, `exp` (1h TTL), `role`, `jti`. Sent as
  `Authorization: Bearer <jwt>`; middleware verifies and injects an `AuthContext`.
- **Roles:** `users.toml` keeps a `role` field (`admin` | `viewer`) for forward compatibility;
  v1 treats all authenticated users identically (read access only — there is nothing else yet).
  (mn-auth's GitHub-OAuth `tier` concept is not carried over; gauge has no OAuth uplift.)

### Rate limiting & config

Per-IP in-memory token buckets (e.g. `governor`); 429 + `Retry-After`. Defaults (env-overridable):
`/v1/logs` 60 req/min/IP (burst 120); `/v1/auth/*` 10 req/min/IP; `/v1/query` + `/v1/meta`
120 req/min per authenticated user.

| Env var | Purpose |
|---|---|
| `DATABASE_URL` | Postgres connection (Fly secret) |
| `GAUGE_JWT_SECRET` | HS256 secret, ≥ 32 bytes (Fly secret) |
| `GAUGE_USER_STORE` | users.toml content (Fly secret) |
| `GAUGE_APP_ALLOWLIST` | Comma-separated allowed `service.name` values |
| `GAUGE_LISTEN_ADDR` | Default `0.0.0.0:8080` |

**Deployment:** Fly app (working name `gauge-telemetry`; fly.dev URL is sufficient for v1 — senders
read the endpoint from their own config), region `lhr`, shared-cpu-1x / 2GB to start,
`min_machines_running = 1`, forced HTTPS, `/readyz` + `/healthz` checks, migrations applied at boot.

## 6. Client design (`gauge` binary)

```
gauge keys generate --user-id <id>   # keypair; private key → ~/.config/gauge/<id>.private (0600)
gauge login                          # challenge/response → JWT cached at ~/.config/gauge/token.json (0600)
gauge tui                            # dashboard
gauge mcp serve                      # MCP server over stdio
gauge query '<json>'                 # one-shot DSL query (scripting/debugging), prints JSON
```

Config at `~/.config/gauge/config.toml`: `server_url`, default `user_id`. (`$XDG_CONFIG_HOME`
respected throughout.)

**Shared API layer** (internal module): async reqwest + rustls; injects the Bearer token; on 401 it
transparently re-runs challenge/response with the local private key, updates the cache, and retries
once — TUI sessions and agent conversations never break on the 1-hour token expiry. Uses
`gauge-query` types for requests/responses and `gauge-auth` for the login flow.

### TUI (ratatui + crossterm + tokio)

Fixed pages in v1:

1. **Overview** — events-over-time braille chart (per-app series); big-number tiles (events today;
   unique installs 24h / 7d / 30d); top event types bar chart; apps summary table (events, uniques,
   last seen).
2. **App detail** (one page per app reported by `/v1/meta`) — event-type breakdown, version distribution,
   os/arch split, per-event-type sparklines.
3. **Explore** — interactive query builder over the DSL: pick measures/dimensions/filters from
   `/v1/meta` values; results as table or bar chart.

Data layer polls `/v1/query` (default every 30s; `r` forces refresh). Keybindings in the
btm/sampler idiom: `q` quit, `tab` switch page, `t` cycle time range (1h/24h/7d/30d), arrows
navigate. Rendering is pure — widgets draw the latest snapshot, so slow networks never block the
UI; failures degrade to a stale-data banner (see §7).

### MCP server (`rmcp`, stdio transport)

Tool input/output schemas are generated from the shared `gauge-query` types via `schemars`, so the
MCP surface and REST API cannot drift.

| Tool | Purpose |
|---|---|
| `query_telemetry` | Full query DSL — arbitrary questions |
| `get_meta` | Discovery: apps, event names, attribute keys — what is askable |
| `unique_users` | Period + optional app/event filters → distinct installs |
| `top_events` | "Most used X": top-N event types by count or uniques |
| `events_over_time` | Timeseries for trend questions |

The three convenience tools are thin wrappers over the DSL; they exist because agents answer
"how many unique users did Y this week?" more reliably from a purpose-named tool than by composing
a query object. When unauthenticated, tools return an MCP error with remediation: "run `gauge login`".

## 7. Error handling

**Server.** One structured JSON error envelope (`code`, `message`, `remediation`) on every
non-OTLP error path. Ingest follows OTLP semantics: malformed body → 400; per-record violations →
partial success with rejected count and reason (values never echoed); unknown app / bad resource
block → 400 whole-batch; DB unavailable → 503 (at-least-once senders retry later). Query validation
errors name the offending field; statement timeout → error with "narrow the time range" remediation.
Auth mapping as in §5. 429 carries `Retry-After`. Request paths never panic; per-crate `thiserror`
types are converted at the tower layer; logs carry request IDs and never payloads.

**Client.** TUI keeps rendering the last good snapshot with a "stale since HH:MM: <reason>" status
bar and retries with backoff. MCP tools return MCP errors with remediation text rather than
protocol failures. CLI subcommands exit nonzero with human-readable messages.

## 8. Testing

| Layer | Strategy |
|---|---|
| `gauge-events` | Golden-file round-trips against pinned OTLP JSON fixtures; profile validation units; SPEC.md worked examples pinned byte-for-byte by test (Tome's TELEMETRY.md-pin pattern) |
| `gauge-auth` | Protocol units: sign/verify, challenge single-use + expiry, JWT mint/verify/tamper/expiry, wire-format parsing (padded/unpadded) |
| `gauge-query` | DSL→SQL snapshot tests; validation rejects unknown fields/measures/dimensions; assertion that every generated statement is fully parameterized |
| `gauge-server` | Integration tests against real Postgres (`sqlx::test`): ingest fixture → query returns expected aggregates; full auth handshake; rate-limit 429; partial-success paths. Privacy canary tests: schema has no IP/UA column; captured tracing output from ingest contains no attribute values |
| `gauge` client | API layer vs mock server (wiremock), incl. 401 → re-auth retry; TUI state/render tests on ratatui `TestBackend`; MCP tool-schema snapshots + in-process tool calls |
| CI | GitHub Actions: fmt, clippy `-D warnings`, tests with Postgres service container, cargo-deny (advisories + licenses) |

## 9. Key dependencies

axum, tokio, tower, sqlx (postgres, runtime-tokio, tls-rustls, migrate), serde/serde_json,
reqwest (rustls, no default features), ed25519-dalek + rand_core/getrandom, jsonwebtoken (HS256),
uuid, time,
governor, thiserror, tracing/tracing-subscriber, ratatui + crossterm, rmcp, schemars;
dev: insta (snapshots), wiremock.

## 10. Future work (explicitly out of v1)

1. **Sender migrations** — Tome and Midnight Manual adopt `gauge-events` (Midnight Manual
   additionally mints an install UUID and retires its own `/v1/telemetry/events` endpoint).
2. Publish `gauge-events` (and possibly `gauge-query`/`gauge-auth`) to crates.io once stable.
3. Rollup/continuous-aggregate tables if query latency degrades with volume.
4. GIN index on `attributes`; table partitioning; retention policy.
5. Configurable TUI dashboards; additional measures (percentiles over numeric attributes).
6. Roles with teeth (admin-only data-management endpoints, e.g. install-id purge for GDPR-style
   requests), token revocation via `jti`.
