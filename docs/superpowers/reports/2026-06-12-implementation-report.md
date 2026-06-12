# Gauge Telemetry Platform — Implementation Report

**Date:** 2026-06-12
**Plan:** `docs/superpowers/plans/2026-06-12-gauge-platform.md`
**Spec:** `docs/superpowers/specs/2026-06-12-gauge-telemetry-platform-design.md`
**Repo:** `github.com/aaronbassett/gauge` (private)
**Execution:** Fully autonomous, subagent-driven (superpowers:subagent-driven-development), no human in the loop.

---

## Outcome

**All 33 tasks across all 3 phases are complete and merged. All 3 phase gates passed. `main` is green.**

- **Tests:** 114 passing (0 failures), `cargo test --workspace --all-features --locked`.
  - gauge-auth 27 · gauge-events 26 (incl. sender) · gauge-query 7 · gauge-server 35 · gauge (client) 19.
- **Gates per task boundary:** `cargo fmt --all --check` + `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` + `cargo test --workspace --all-features --locked`, all green.
- **No test was ever weakened, ignored, or deleted.** The two privacy canaries and the SPEC.md pin test pass unmodified (the log-leak canary was *hardened* with a non-vacuous-capture assertion).
- **End-to-end verified locally** against a real gauge-server + Postgres: ed25519 login handshake → authenticated query; and `gauge_events::sender` enqueue → drain → queryable via the TUI/MCP path.

---

## What was built

A five-crate Cargo workspace (edition 2024, resolver 3, Rust 1.93):

| Crate | Delivers |
|---|---|
| `gauge-auth` | ed25519 keypair + `ed25519:<base64>` wire format, TOML user store, single-use 60s challenge store, HS256 JWT mint/verify (redacted secret), client sign helper |
| `gauge-events` | OTLP logs-signal serde types, Gauge-profile validation (partial rejection), pinned `SPEC.md`; **Phase 3:** feature-gated `sender` (disk queue + crash-safe at-least-once drain) |
| `gauge-query` | Typed query DSL (`Field` enum where `install_id`/`session_id` are deliberately not addressable), validation (limit cap, time-range grammar, filter op/value rules) |
| `gauge-server` | axum 0.8 server: OTLP ingest, ed25519 challenge/verify auth, bearer middleware, DSL→parameterized-SQL builder, `/v1/query` + `/v1/meta`, per-IP/per-user rate limiting, privacy canaries; sqlx 0.8 + Postgres (single JSONB-attributed `events` table) |
| `gauge` | Client binary: `keys generate` (0600 seed), `login` (token cache + 401 retry), `query`, `tui` (ratatui dashboard), `mcp serve` (rmcp stdio, 5 anonymity-preserving tools) |

Plus deployment artifacts (`Dockerfile.server` → 60.3 MB distroless, `fly.toml`, `docs/deploy.md`) and CI (`lint` / `test` w/ Postgres service / `deny`).

---

## PRs merged (13)

| PR | Tasks | Branch |
|---|---|---|
| #1 | 1 — workspace scaffold + CI | feat/task-01-scaffold |
| #2 | 2–6 — gauge-auth | feat/auth |
| #3 | 7–9 — gauge-events (Phase 1) | feat/events |
| #4 | 10 — gauge-query DSL | feat/query |
| #5 | 11–12 — server foundation | feat/server-foundation |
| #6 | 13–15 — ingest + auth + bearer | feat/server-endpoints |
| #7 | 16–18 — query builder + /v1/query + /v1/meta | feat/server-query |
| #8 | 19–20 — rate limiting + privacy canaries | feat/server-ratelimit-canary |
| #9 | 21 — deployment artifacts | feat/deploy |
| #10 | 22–24 — client foundation (CLI, keys, ApiClient) | feat/client-foundation |
| #11 | 25–27 — query cmd + MCP server | feat/client-mcp |
| #12 | 28–30 — TUI dashboard | feat/client-tui |
| #13 | 31–33 — sender | feat/sender |

Each PR: dispatched a fresh implementer subagent (given the exact plan line-range), self-reviewed the diff + ran the full gate, security-reviewed the auth/crypto-bearing PRs with a dedicated reviewer subagent, watched CI to green, then squash-merged with branch deletion. All feature branches deleted; only `origin/main` remains.

---

## Dependency drift & engineering decisions

1. **`time` pinned to `=0.3.47`** (workspace). `time 0.3.48` (edition 2024) adds a blanket impl that conflicts (E0119) with `sqlx-core 0.8.6`'s `impl<T> From<T> for Json<T>`. The pin keeps `sqlx 0.8` and the `rust-version 1.93` floor. *Revisit if sqlx ships an 0.8.x patch, or move to sqlx 0.9 (needs Rust ≥ 1.94).*

2. **rmcp `0.6` → `1.7.0`** (Task 27). The plan pinned `rmcp = "0.6"`. The implementer initially claimed 0.6 was unavailable — **that was false** (0.6.4 is unyanked and schemars-1 compatible). The deliberate decision was to move to **rmcp 1.7.0, the current stable major** (the plan pinned 0.6 only as "current at plan time", and its own drift note authorizes adapting). 5 symbols adapted (router/wrapper paths, `#[tool_handler(router=…)]`, `ServerInfo::new(...).with_instructions(...)`). Verified via a real stdio `initialize`+`tools/list` smoke. The 5 tools and their anonymity language are intact.

3. **`Cargo.lock` drift + CI hardening.** PR #11 merged a `Cargo.toml` change without the regenerated `Cargo.lock` (subagent `git add`ed only the crate dir). CI was green because it didn't pass `--locked`. Fixed the lock on `main` and **added `--locked` to the CI clippy+test steps** so this class of bug now fails CI. Subsequent dep-changing tasks committed the lock correctly.

4. **cargo-deny: `paste` unmaintained** (RUSTSEC-2024-0436), pulled in transitively by ratatui 0.29. `paste` is feature-complete/deprecated (not a vulnerability), no safe upgrade. Added a **scoped** `ignore = ["RUSTSEC-2024-0436"]` (single ID; other unmaintained/vulnerable crates still fail the gate).

5. **No drift** in axum 0.8, sqlx 0.8, tower-http 0.6, tracing-subscriber 0.3, schemars 1, insta 1, ratatui 0.29, crossterm 0.28, reqwest 0.12, wiremock 0.6, jsonwebtoken 9, ed25519-dalek 2 — the plan's code compiled verbatim.

6. **Minor TDD-driven adaptations:** `#[derive(Debug)]` on `UserStore` (test `.unwrap_err()`); `tokio::sync::Mutex` for async-test env locks (clippy `await_holding_lock`); `impl Default for App` (clippy `new_without_default`); `main.rs` error boxing. All preserve test intent.

7. **Review-driven hardening (no behavior change to approved design):** documented `Keypair::seed()` as enrollment-only/non-logging; added invalid-curve-point, schema_version=0, expired-challenge-401, empty-bearer-token, and exists-filter-end-to-end pin tests; fixed a latent redundant bind in the SQL builder's `exists` path (was harmless only because PG tolerates an explicit-typed unused param); hardened the log-leak canary against vacuous passes.

---

## Deferred to human

**Live `fly deploy` (the single permitted deferral).** `fly` is authenticated locally, but `fly deploy` provisions billable infrastructure (Fly Managed Postgres + an always-on machine) and a public endpoint — an outward-facing, hard-to-reverse, billing-bearing action that should be a human's call. **Deploy-readiness was fully verified locally**: the Docker image builds (60.3 MB distroless); the release binary boots against Postgres, runs migrations, and serves `/healthz`=200, `/readyz`=200, ingests the SPEC worked example (200, `accepted:1`), and 401s unauthenticated `/v1/query`.

**To go live**, a human should run `docs/deploy.md`:
1. `fly apps create gauge-telemetry`; provision + attach Managed Postgres (sets `DATABASE_URL`).
2. `fly secrets set GAUGE_JWT_SECRET=… GAUGE_APP_ALLOWLIST="tome,midnight-manual" GAUGE_USER_STORE="$(cat users.toml)"` — where `users.toml` registers admin public keys from `gauge keys generate`.
3. `fly deploy`; verify `/healthz` + a curl OTLP POST + a full `gauge login` handshake.

---

## Recommended next steps for the Tome / Midnight Manual migration

The `gauge_events::sender` (Phase 3) is the migration vehicle. Wiring a new app:

1. **Depend on `gauge-events` with the `sender` feature.** Build a `SenderConfig` once per process (`app`, `app_version`, a persistent per-install `install_id` UUID, a per-run `session_id` UUID, `os`/`arch`, and a `queue_path` on local disk).
2. **`enqueue(&cfg, "app.event_name", attrs)`** on each telemetry event (event names MUST be prefixed `"{app}."`; attributes are bounded — ≤30/record, ≤128 bytes/string). Enqueue is cheap and crash-safe (bounded disk queue, oversized/full lines dropped, never blocks the app).
3. **`drain(&cfg)`** on a timer / at shutdown. It POSTs in ≤100-event batches, removes lines only after a 2xx (at-least-once; a crash mid-drain re-sends), and is single-flighted via a lock file.

**Critical gotchas (learned in the Phase Gate 3 E2E):**
- **`SenderConfig.endpoint` is the BASE URL** (e.g. `https://gauge-telemetry.fly.dev`). `transport::post_batch` appends `/v1/logs` itself — passing a full path yields `…/v1/logs/v1/logs` → 404 → `drain` silently returns `sent:0` and re-queues forever.
- **`endpoint_allowed` requires `https://`** (or loopback for local dev), so production senders fail-closed unless pointed at the deployed HTTPS endpoint.
- **The `sender_drain` unit tests use wiremock**, which 200s any body and therefore does NOT validate the encoded batch against the server's Gauge-profile validation. **Always do a real-server smoke** (enqueue → drain → confirm via `gauge query`/TUI) when onboarding a new app, exactly as this gate did.

**Server-side onboarding:** add the app to `GAUGE_APP_ALLOWLIST`; events from unknown apps are rejected whole. The Gauge profile requires `service.name/version`, `service.instance.id` (UUID), `session.id` (UUID), `os.type` ∈ {darwin,linux,windows}, `host.arch` ∈ {amd64,arm64}.

**Querying the data:** `gauge tui` for the dashboard, `gauge query '<json>'` for one-shots, or wire the MCP server (`gauge mcp serve`) into Claude for natural-language analytics (`unique_users`, `top_events`, `events_over_time`, `query_telemetry`, `get_meta`). Remember the privacy model: there is **no** per-user drill-down — only aggregate counts and unique-install/session counts.

**Suggested future hardening (out of scope here, recorded for follow-up):**
- Return `Keypair::seed()` as `zeroize::Zeroizing<[u8;32]>` (deferred to avoid rippling into the `gauge keys` `[u8;32]` consumers).
- Consider upgrading ratatui 0.29 → 0.30 (drops the unmaintained `paste`, lets the deny ignore be removed) and rmcp is already on current 1.x.
- Add `Swatinem/rust-cache` to CI to cut per-run compile time (~2–3.5 min cold).
