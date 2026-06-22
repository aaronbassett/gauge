# `gauge status` and `gauge version` commands — Design

- **Date:** 2026-06-22
- **Status:** Approved design; ready for implementation planning.
- **Issue:** None yet — to be opened during implementation planning.
- **Inspiration:** `tome status` (`crates/.../commands/status.rs` in the Tome repo) —
  same left-art + right-panel + "Overall:" layout, adapted to gauge's
  thin-client-to-remote-server shape.

---

## 1. Context & problem

`gauge` (crate `gauge-client`, binary `gauge`) is a thin client to a remote
`gauge-server`. Today its subcommands are `keys`, `login`, `query`, `tui`, and
`mcp serve` (`crates/gauge/src/main.rs`). There is:

- **No `status` command.** A user has no single place to answer "is my client
  configured, can it reach the server, am I authenticated, and is there data?"
- **No `version` command and no `--version` flag.** clap's derive does not
  auto-supply `--version` unless asked, and there is no bare-version output for
  scripts/Homebrew/CI.

We add both, modelled on Tome's `status` (left-side art + right-side info panel
grouped into sections, ending in an `Overall:` health line, with `--json` and
exit-code semantics).

### Current shape (relevant facts)

- **Local client state:** `ClientConfig { server_url, user_id }` from
  `~/.config/gauge/config.toml` (`config.rs`, `paths.rs`); an Ed25519 private
  key at `<config_dir>/<user_id>.private` (`keys.rs`); a cached JWT at
  `<config_dir>/token.json` as `TokenCache { token, user_id, expires_at }`
  (`api.rs`).
- **Server endpoints:** `GET /healthz` (unauthed → `"ok"`), `GET /readyz`
  (unauthed → checks DB), `GET /v1/meta` (authed → `MetaResponse { apps:
  Vec<AppMeta> }`, each `AppMeta` carrying `total_events: i64` and `last_event:
  Option<String>` RFC3339, among others). The server exposes **no** version
  endpoint.
- **No plain-text color helper exists.** Color lives only inside the ratatui TUI
  (`tui/theme.rs` `Palette`/`builtin_palette`). CLI subcommands use bare
  `println!`.
- **Test tooling:** the `gauge` crate dev-deps are `wiremock` + `tempfile`; the
  workspace also provides `insta`.

---

## 2. Goals / non-goals

**Goals**

1. `gauge version` subcommand **and** `--version`/`-V` flag that print **only**
   the bare version (`0.3.0\n`) to stdout and nothing else.
2. `gauge status` that reports client config/auth state, server reachability +
   DB readiness, and a data overview — server-aware, but degrading gracefully
   when offline/unauthenticated (never panics, never propagates a network error
   as a failure exit before rendering).
3. `gauge status --json` emitting a stable structured report.
4. A left-side **sparkline-wave** art block + right-side panel, matching the
   Tome layout, with the art colored from the same palette `gauge tui` uses.
5. Exit codes: `healthy → 0`, `degraded → 1`, `unhealthy → 1` (Tome parity).

**Non-goals**

- No `--verify`-style deep checks (Tome rehashes model files; gauge has no such
  local artefacts).
- No `--json` for `version` (the request is the bare number, nothing else).
- No new server endpoint (no server version surfaced; only client version).
- No global `--json` flag — `--json` is local to `status` (the only
  human-vs-machine-dual command; `query` already always emits JSON).
- No forced login. `status` inspects/uses existing credentials best-effort; it
  does not interactively prompt or hard-fail on missing keys.

---

## 3. Decisions (locked, from brainstorming)

1. **Scope: network with graceful degradation.** `status` probes the server
   (`/healthz`, `/readyz`, `/v1/meta`); failures become report fields, not
   errors.
2. **Art: sparkline wave.** A framed flowing waveform, evoking a live event
   stream, colored per-column from the palette.
3. **Exit codes: Tome parity.** `healthy → 0`, otherwise `1`. The report renders
   fully (human or JSON) **before** any `std::process::exit`.
4. **Sparkline colors: reuse the TUI palette.** Resolve the same palette
   `gauge tui` uses (default `tokyo-night`, honoring a custom `dashboard.toml`),
   convert `ratatui::Color` → ANSI for stdout. Fall back to a fixed built-in
   accent ramp if palette resolution fails for any reason (status must always
   render).

---

## 4. Architecture — module structure

New files in `crates/gauge`:

```
src/
  status/
    mod.rs     # StatusReport types, async assemble_report, classify,
               # emit (human panel + JSON). Public assemble_* entry point
               # tests call directly (no process::exit inside it).
    art.rs     # sparkline-wave art: ART_WIDTH, sparkline(), per-column paint.
  term.rs      # NEW dependency-free terminal helpers:
               #   stdout_is_tty(), color gating (TTY && !NO_COLOR),
               #   success/warning/error/label/dim/bold ANSI wrappers,
               #   ratatui::Color -> ANSI(truecolor/named) conversion,
               #   term_width() ($COLUMNS else 80).
```

Edited files:

- `lib.rs` — add `pub mod status;` and `pub mod term;`.
- `main.rs` — add `Status`/`Version` subcommands, the `--version`/`-V`
  pre-parse hook (`disable_version_flag = true`), and dispatch.

Rationale: a `status/` directory (vs one file) keeps the art separate from the
assembly/render logic and mirrors Tome, so the two stay easy to compare. `term`
is a standalone primitive because color/TTY handling is reusable beyond status
and shouldn't live inside the `status` module.

---

## 5. `version` / `--version`

- clap derive on the top-level `Cli` gets `disable_version_flag = true`.
- `main()` intercepts **before** `Cli::parse()`:
  ```rust
  let raw: Vec<String> = std::env::args().collect();
  if raw.iter().skip(1).any(|a| a == "--version" || a == "-V") {
      println!("{}", env!("CARGO_PKG_VERSION"));
      std::process::exit(0);
  }
  ```
- A `Version` subcommand prints the identical bare string.
- Output is exactly `0.3.0\n` — no `gauge ` prefix, no model lines (gauge has no
  models), no JSON. This differs from Tome's *extended* `--version`; gauge's is
  deliberately minimal per the request.

---

## 6. `status` — probes (all best-effort, caught per-probe)

Entry point: `assemble_report(config: Result<ClientConfig, ClientError>) ->
StatusReport`, `async` and **infallible at the report level** — every probe maps
failure to a report field. It owns the orchestration: when `config` is `Err`
(missing/invalid), `config_loaded = false` and **no `ApiClient` is built** — all
network probes short-circuit to `unreachable`/`unavailable`. When `config` is
`Ok`, it constructs the short-timeout `ApiClient` internally from `server_url`
and runs the probes below. (`run()` does the `ClientConfig::load()` call and
passes the `Result` in; tests can pass either arm directly.) Probes:

1. **Client (local, no I/O beyond fs):**
   - `config_path` (from `paths::config_path()`), `config_loaded` (did
     `ClientConfig::load()` succeed?), `server_url`, `user_id`.
   - `key_present`: does `paths::key_path(user_id)` exist?
   - `token`: read `token.json`; report `present`, `valid` (matches `user_id`
     and `expires_at > now`), `expires_at`, `expires_in_secs` (may be negative
     → rendered "expired").
2. **Server (network, unauthed, short timeout ~4s):**
   - `GET /healthz` → `reachable` (200/`"ok"`), else `error` = reason
     (timeout / connection refused / non-200).
   - `GET /readyz` → `db_ready`.
3. **Data (network, authed, best-effort):**
   - `GET /v1/meta` via the existing token flow. On success: `apps =
     meta.apps.len()`, `total_events = Σ total_events`, `last_event = max
     last_event`, plus `per_app` detail (JSON only). On any failure (no key,
     auth rejected, network): `available = false`, `error` = reason.

A dedicated short-timeout `reqwest::Client` is used for the status probes so the
command stays snappy even when the server is down (the default `ApiClient`
timeout is 10s; status uses ~4s). Probe 3 reuses `ApiClient` so token caching /
silent re-login behave exactly as `query`/`tui` would.

---

## 7. Health classification + exit code

| Overall     | Condition                                                                 | Exit |
|-------------|---------------------------------------------------------------------------|------|
| `unhealthy` | config missing/invalid, **or** server unreachable, **or** `/readyz` fails | 1    |
| `degraded`  | server reachable + DB ready, but data unavailable (no key / token invalid / auth failed) | 1 |
| `healthy`   | config loaded · server reachable · DB ready · meta fetched                | 0    |

`run()` calls `emit()` first, then `std::process::exit(1)` for non-`healthy`.
`assemble_report()` itself never exits — library/integration tests call it and
assert on the returned struct. (Same split Tome uses to keep the report
testable.)

---

## 8. Panel layout (human)

```
┌───────────────────────┐   Gauge v0.3.0
│        ╱╲    ╱╲        │
│   ╱╲╱╲╱  ╲╱╲╱  ╲╱╲     │   Client
│ ╱╲              ╲╱╲    │   Config:     ~/.config/gauge/config.toml
└───────────────────────┘   User:       aaron
                            Key:        ✓ present
                            Token:      ✓ valid · expires in 41m

                            Server
                            Endpoint:   https://gauge.fly.dev
                            Reachable:  ✓ ok · DB ✓ ready
                            Apps:       3 · 1.2M events
                            Latest:     4m ago

                            Overall:    ✓ healthy
```

- Header: `Gauge v{CARGO_PKG_VERSION}` (bold).
- Section labels (`Client`, `Server`) dimmed; field keys left-padded to a fixed
  column then colored (padding inside the color span, so ANSI codes don't break
  alignment — Tome's trick).
- Glyphs: `✓` (success/green), `⚠` (warning/yellow), `✗` (error/red); plain
  fallbacks `[ok]`/`[warn]`/`[fail]` when color is disabled.
- **Degraded examples:**
  - server up, not authed → `Apps: unauthenticated`, `Overall: ⚠ degraded`.
  - token expired → `Token: ⚠ expired`.
- **Unhealthy example:** server down → `Reachable: ✗ unreachable (connection
  refused)`, `Apps`/`Latest` omitted, `Overall: ✗ unhealthy`.
- **Art gating:** show art only when `stdout_is_tty()` **and** `term_width() >=
  ART_WIDTH + GAP + PANEL_MIN`; otherwise panel-only. Non-TTY ⇒ color already
  off ⇒ plain panel (pipe-friendly). Identical strategy to Tome.
- Counts humanized: `1.2M events`, `784.4 KiB`-style for any byte sizes,
  relative times (`just now` / `Nm ago` / `Nh ago` / `Nd ago`).

---

## 9. `status --json` shape

Stable, snapshot-tested. `null`able `error` fields explain degradations.

```json
{
  "gauge": "0.3.0",
  "client": {
    "config_path": "/Users/aaron/.config/gauge/config.toml",
    "config_loaded": true,
    "server_url": "https://gauge.fly.dev",
    "user_id": "aaron",
    "key_present": true,
    "token": { "present": true, "valid": true,
               "expires_at": 1750600000, "expires_in_secs": 2460 }
  },
  "server": { "endpoint": "https://gauge.fly.dev",
              "reachable": true, "db_ready": true, "error": null },
  "data": {
    "available": true,
    "apps": 3,
    "total_events": 1200000,
    "last_event": "2026-06-22T10:25:00Z",
    "per_app": [ { "app": "tome", "total_events": 800000,
                   "last_event": "2026-06-22T10:25:00Z" } ],
    "error": null
  },
  "overall": "healthy"
}
```

- `overall` is `"healthy" | "degraded" | "unhealthy"` (lowercase, like Tome's
  `OverallHealth` serde rename).
- When `config_loaded == false`: `server`/`data` are still present but report
  `reachable: false` / `available: false` with explanatory `error`s, and
  `overall` is `unhealthy`.
- JSON is emitted to stdout, then the same non-zero exit applies.

---

## 10. Error handling & edge cases

- **No config file:** `config_loaded = false`, `server_url`/`user_id` empty,
  every network probe short-circuits to `unreachable`/`unavailable`, `overall =
  unhealthy`. The human panel still renders with a `Config: missing — create
  ~/.config/gauge/config.toml` hint.
- **No HOME / no config dir:** `paths::config_dir()` errors → treat as config
  missing (same as above); never panic.
- **Server reachable but `/v1/meta` 401 (token mintable):** `ApiClient` will
  silently attempt re-login via the key; if that succeeds, data is available
  (healthy). If there is no key, it fails → `data.error = "unauthenticated"`,
  degraded.
- **Clock skew (negative `expires_in_secs`):** render "expired", `valid =
  false`.
- **Palette resolution failure:** fall back to the fixed accent ramp; never
  abort the render.
- **Narrow / piped output:** art suppressed, plain panel only.

---

## 11. Testing

- **Unit (`status/mod.rs`, `term.rs`, `art.rs`):**
  - `classify()` truth table for all three overall states.
  - expiry formatting (valid / expiring soon / expired / clock skew),
    relative-time buckets, number/byte humanization.
  - art: every `sparkline()` line is exactly `ART_WIDTH` visible chars; paint
    leaves width unchanged; plain (color-off) form contains no ANSI.
  - `term`: color gating off under `NO_COLOR` / non-TTY; `Color::Rgb` →
    truecolor escape; named/`Reset` colors map sanely.
- **Integration (`tests/status.rs`, `wiremock` + `tempfile`):**
  - Healthy: mock `/healthz`, `/readyz`, `/v1/meta`; point a temp
    `GAUGE_CONFIG_DIR` with a config + key + token; assert assembled
    `StatusReport` + `overall == healthy`.
  - Server down: no mock / 503 → `reachable == false`, `overall == unhealthy`.
  - Unauthenticated: healthz/readyz ok, `/v1/meta` 401 + no key → `overall ==
    degraded`.
  - `--json` snapshot (insta) for the healthy case (redact the volatile
    `config_path`/`expires_*` fields or pin them via fixtures).
  - Bare version: assert running the binary with `--version` and the `version`
    subcommand both print exactly `0.3.0\n`.

---

## 12. Open questions

None blocking. Possible follow-ups (out of scope here): surfacing a server
version once the server exposes one; a `status --watch` live mode; per-app
breakdown in the human panel (currently JSON-only).
