# Configurable, themed, filterable TUI dashboard — Design

- **Date:** 2026-06-18
- **Status:** Approved design; ready for implementation planning.
- **Issue:** None yet — to be opened during implementation planning.
- **Companion:** [Read-time numeric bucketing, aggregates & filters](2026-06-17-numeric-query-dsl-design.md)
  (that design added the numeric measures/filters/buckets this dashboard surfaces;
  this design is the client-side half that finally exposes the DSL's full power in the TUI).

---

## 1. Context & problem

`gauge tui` today is a fixed three-page terminal app (`Overview`, `Apps`, `Explore`)
built on `ratatui 0.29`. It renders a small, hardcoded set of widgets, uses five
hardcoded series colours, square borders, and exposes **no filtering except the time
window** (`1h`/`24h`/`7d`/`30d`). It is functional but visually plain and not
customizable.

Meanwhile the query DSL (`gauge-query`) already supports **rich filtering**
(`eq`/`neq`/`in`/`exists`/`gt`/`gte`/`lt`/`lte` over `app`, `event_name`, `os`,
`arch`, `app_version`, and any `attr.<key>`), **numeric aggregates** (`avg`/`min`/
`max`/`p50`/`p90`/`p95`/`p99`), **numeric bucket dimensions**, and **absolute time
ranges**. The TUI surfaces almost none of this. The goal is a dashboard that is
*more useful*, *more customizable*, and *more visually appealing* — closer to the
density and polish of `sampler` and `btop` — by exposing capability that already
exists server-side.

### Current shape (as of `048c468`)

- `tui/app.rs` — `Page{Overview,Apps,Explore}` enum, `App` state, `on_key`, a
  hardcoded Explore picker (`EXPLORE_MEASURES`/`EXPLORE_DIMENSIONS`).
- `tui/data.rs` — `TimeWindow` enum; `Snapshot` (fixed fields); `fetch()` runs three
  fixed queries + `/v1/meta` sequentially; histogram probe/bucket helpers.
- `tui/ui.rs` — per-page render fns; hardcoded `SERIES_COLORS`; square `Borders::ALL`.
- `tui/run.rs` — event loop: `crossterm` `EventStream` + `tokio::select!`, an mpsc
  `Msg` channel, a 30s poll tick, pure render decoupled from polling, stale banner.
- `api.rs` — `query()` / `meta()`; transparent 401 re-login + one retry.
- `config.rs` — `ClientConfig { server_url, user_id }` from `config.toml`.

## 2. Goals / non-goals

**Goals**
- One **configurable dashboard** mode: a dense grid of panels defined declaratively,
  arrangeable live, replacing the fixed `Overview`/`Apps` pages.
- A **global live filter bar** over every non-identifying field, plus optional
  per-panel static filter pins from config.
- A **theme system**: built-in palettes (Tokyo Night default), an ANSI mode, and
  custom palettes; rounded borders + gradient meters as defaults.
- **Customization via both** a `dashboard.toml` file *and* a live in-app menu whose
  edits persist back to that file.
- Keep `Explore` as a second mode for ad-hoc DSL queries (re-themed).
- Preserve the decoupled-polling / pure-render architecture and the stale-data UX.

**Non-goals**
- **No changes** to `gauge-query`, `gauge-server`, the MCP surface, auth, or
  `config.toml`. This is entirely client-side.
- No new heavy dependencies (`ratatui 0.29`, `toml`, `serde` already present).
- No weakening of privacy: aggregates only; `install_id`/`session_id` stay
  non-addressable (so they can never become a filter or group-by).
- No absolute / custom date-range *picker* UI in this iteration (the global window
  stays the existing `1h`/`24h`/`7d`/`30d` cycle; absolute ranges are computed
  internally only for period-over-period deltas).
- No per-app detail *page* — per-app health is served by the optional `apps_table`
  panel and the `app` filter.

## 3. Decisions (locked, from brainstorming)

| # | Decision | Rationale |
|---|---|---|
| 1 | **Approach A**: one configurable dense dashboard + kept `Explore` mode; presets switch whole layouts (tab-like). | Only option delivering both sampler density and btop live customization; presets subsume a tabbed model. |
| 2 | Default dashboard prioritizes **adoption/growth, feature usage, quality/perf**; per-app health is secondary. | User-selected dashboard jobs. |
| 3 | Customization is **both** declarative (`dashboard.toml`) **and** live in-app, with live edits **persisted back** to the file (atomic write). | Shareable defaults + live control; matches both reference tools. |
| 4 | Filtering is a **global live filter bar** applied to every panel, **plus per-panel static pins** defined in config. | Simple mental model with room for comparison-style slicing. |
| 5 | Default theme **Tokyo Night**; ship Catppuccin Mocha, Gruvbox Dark, Nord, ANSI; custom palettes via config. **Rounded borders + gradient meters** default. | User-selected; themeable system replaces hardcoded colours. |
| 6 | Panels are a **trait + factory registry** (one focused unit per kind), not a closed enum. | Independently testable units; open to new panel kinds. |
| 7 | **Per-panel** error/loading/stale states; global banner only on whole-fetch failure. | One failing query must not blank the dashboard. |

## 4. Architecture — modes & module structure

`Page{Overview,Apps,Explore}` is replaced by two **modes**: `Dashboard` (default)
and `Explore` (the existing query builder, re-themed). New module layout under
`crates/gauge/src/tui/`:

```
tui/
  run.rs        event loop (kept: mpsc + tokio::select! + pure render + 30s tick)
  app.rs        App state, Mode enum, key dispatch, overlay state
  theme.rs      Theme palette + built-ins + ANSI + custom; border/meter style
  config.rs     DashboardConfig (dashboard.toml): theme, presets, panels; default
  layout.rs     span-based row-flow solver → Vec<Rect> over a 12-col grid
  filter.rs     global filter-bar state, value discovery from meta, chip rendering
  menu.rs       live menu overlay (preset/panel/theme edits) + persistence
  data.rs       request collection, dedup, concurrent fetch, ResultMap
  panels/
    mod.rs      Panel trait + PanelCtx + build(spec) factory (the registry)
    timeseries.rs  stat.rs  top_n.rs  breakdown.rs
    numeric_stats.rs  histogram.rs  apps_table.rs
```

The client config module (`config.rs` for `ClientConfig`) is unchanged; the new
dashboard config is a separate concern. (If naming collides, the dashboard config
lives in `tui/config.rs` while `ClientConfig` stays in the crate-root `config.rs`.)

## 5. The Panel abstraction

Each panel kind is a small unit implementing one trait:

```rust
pub struct PanelCtx<'a> {
    pub window: TimeWindow,
    pub filters: &'a [Filter],   // global filter bar
    pub meta: &'a [AppMeta],     // from /v1/meta, for value discovery / attr resolution
}

pub struct LabeledRequest { pub key: RequestKey, pub request: QueryRequest }

pub trait Panel {
    fn title(&self) -> String;
    /// Queries this panel needs for the given context. Global filters are merged in
    /// by the caller; the panel appends its own config pins.
    fn data_requests(&self, ctx: &PanelCtx) -> Vec<LabeledRequest>;
    fn render(&self, f: &mut Frame, area: Rect, results: &ResultMap, theme: &Theme);
}

/// Map a config `kind` string + options → a boxed panel. Adding a kind = new file +
/// one arm here.
pub fn build(spec: &PanelSpec) -> Result<Box<dyn Panel>, ConfigError>;
```

Notable panels:
- **`stat`** emits *three* requests: current-window count, previous-window count
  (an `Absolute` range computed from now) for the ▲▼ delta, and a small granular
  timeseries for the sparkline.
- **`timeseries`** emits one grouped, granular query (metric × optional group-by).
- **`numeric_stats`/`histogram`** resolve their numeric attr from config, else the
  first `numeric_attribute_keys` in meta; if none exist they render a placeholder.

`RequestKey` is the hash of the serialized `QueryRequest`, so identical requests
from different panels are fetched once.

## 6. Data flow

1. On refresh, `data.rs` walks every **visible** panel, calls `data_requests`,
   merges the **global filters** into each request and appends **per-panel pins**.
2. All `LabeledRequest`s are **deduped by `RequestKey`** and fetched **concurrently**
   (replacing the current sequential `fetch()`), producing
   `ResultMap = HashMap<RequestKey, Result<QueryResponse, String>>`.
3. `/v1/meta` is fetched alongside (drives filter value discovery + attr resolution).
4. The whole bundle is sent as one message over the existing mpsc channel; the event
   loop stores it and re-renders. Polling cadence (30s, `r` to force) and the stale
   banner are unchanged. The transparent 401 re-login in `api.rs` is unchanged.

Panels render purely from the `ResultMap` by key — a missing or `Err` key renders
that panel's own error/empty state without affecting siblings.

## 7. Filtering

- `App.filters: Vec<Filter>` backs a top **chip bar** (`field op value`).
- **`/`** opens filter entry: choose **field** → **op** → **value**. Enumerable
  fields (`app`, `event_name`, `os`, `arch`, `app_version`) suggest values from
  `/v1/meta`; `attr.<key>` values are free-form (numeric for comparison ops).
  **`c`** clears all filters. Each change triggers a refresh.
- **Per-panel pins**: `[[preset.panel]]` entries may carry their own `filters`,
  merged with (and additive to) the global set at query-build time.
- **Privacy guard:** filter fields go through `Field::parse`, which has no variant
  for `install_id`/`session_id`, so identifying fields cannot be entered. A unit
  test pins this.

## 8. Customization — config + live, persisted

**`dashboard.toml`** lives in the same dir as `config.toml` (XDG /
`GAUGE_CONFIG_DIR`). Absent → a built-in default config (Section 10) loads in memory,
so the dashboard works out of the box. Shape:

```toml
[theme]
name    = "tokyo-night"   # built-in name, "ansi", or "custom"
borders = "rounded"       # rounded | square
meters  = "gradient"      # gradient | solid

active_preset = "default"

[[preset]]
name = "default"

  [[preset.panel]]
  kind    = "timeseries"
  metrics = ["events", "unique_installs", "unique_sessions"]
  span    = 12
  height  = 8

  [[preset.panel]]
  kind   = "stat"
  metric = "events"
  span   = 3

  [[preset.panel]]
  kind   = "top_n"
  field  = "event_name"
  measure = "count"
  limit  = 5
  span   = 6
  filters = [ { field = "app", op = "eq", value = "tome" } ]  # optional pin

# `span` is 1–12 grid columns (panels flow left-to-right, wrapping to a new row on
# overflow). `height` is terminal rows for that row; omit or set `height = "fill"` to
# share remaining vertical space evenly. Panels on the same row share the tallest.

# optional custom palette
# [theme.palette]
# bg = "#1a1b26"  fg = "#c0caf5"  accent = ["#7aa2f7", "#7dcfff", ...]  ...
```

**Live menu (`m`)** edits the in-memory `DashboardConfig`: switch preset, toggle /
add / remove / reorder panels, change theme + border/meter style. **`p`** cycles
presets. Edits **persist back** to `dashboard.toml` via atomic write (temp file +
rename, mode `0644` — not secret). `t` cycles the global window; `tab` toggles
Dashboard ⇄ Explore; `?` help; `q` quit.

## 9. Theme system

`Theme` carries a palette — `bg`, `surface`, `text`, `muted`, and an **accent ramp**
(series colours + gradient endpoints) — plus two style flags (`borders`,
`meters`). Built-ins: **`tokyo-night` (default)**, `catppuccin-mocha`,
`gruvbox-dark`, `nord`, and **`ansi`** (maps to the terminal's 16 colours). A
`[theme.palette]` table overrides any of these. This replaces the hardcoded
`SERIES_COLORS` and `Borders::ALL`; rounded borders use `ratatui`'s
`BorderType::Rounded`, gradient meters interpolate across the accent ramp.

## 10. Default dashboard (shipped config)

The approved layout, top to bottom (12-col grid):

```
 gauge ▸ dashboard       preset: default ▾                 24h ▾
 filters: (none)                                   /:add   c:clear
┌ Activity — events · installs · sessions over time ──────────────┐  timeseries span12
│  multi-series braille; legend shows current + ▲▼ vs prev period  │
└──────────────────────────────────────────────────────────────────┘
┌ events ───┐┌ installs ─┐┌ sessions ─┐┌ p95 latency ─┐            4× stat span3
│  12.4k    ││   318     ││  1.21k    ││   142 ms      │  (delta + sparkline)
└───────────┘└───────────┘└───────────┘└───────────────┘
┌ Top events ────────────────┐┌ Latency distribution ───────────┐  top_n span6 +
│ build  ▇▇▇▇▇▇▇   4,210     ││ p50 38  p90 410  p99 612 + hist  │  numeric_stats/
└────────────────────────────┘└──────────────────────────────────┘  histogram span6
┌ OS ────────┐┌ Arch ───────┐┌ Versions ────────────────────────┐  3× breakdown
│ linux  61% ││ x86_64  72% ││ 0.4.2 ▇▇▇▇▇  0.4.1 ▇▇  0.3.x ▇    │
└────────────┘└─────────────┘└───────────────────────────────────┘
 tab:explore   /:filter   m:menu   p:preset   t:range   ?:help   q:quit
```

**Panel catalog** (build-registry kinds):

| kind | category | summary |
|---|---|---|
| `timeseries` | growth | multi-series line over time; metric(s) + optional group-by |
| `stat` | growth | big number + ▲▼ delta vs previous period + sparkline |
| `top_n` | usage | ranked horizontal bars for a dimension by count/uniques |
| `breakdown` | usage | share-of-total % bars for a dimension (os/arch/version) |
| `numeric_stats` | quality | p50/p90/p95/p99 + min/max/avg for a numeric attr |
| `histogram` | quality | bucketed distribution of a numeric attr (auto-fit edges) |
| `apps_table` | secondary | per-app totals / types / last-seen (per-app health, on demand) |

The bottom OS/Arch/Versions row is kept in the default per user confirmation.

## 11. Error handling & edge cases

- **Per-panel failure:** an `Err` result key → that panel shows an inline error/“—”;
  siblings stay live. **Whole-fetch failure** (network/auth) → global stale banner,
  last good data retained.
- **Missing numeric attr:** `numeric_stats`/`histogram` auto-resolve to the first
  numeric attr in meta; none present → friendly placeholder, not an error.
- **Empty result / loading:** per-panel "loading…" then "no data" states.
- **Invalid `dashboard.toml`:** fall back to the built-in default and surface a
  one-line config-error banner (never crash); the live menu can then rewrite a valid
  file.
- **Degenerate ranges / unknown filter values:** existing DSL/validate behaviour and
  the histogram `derive_edges` degenerate handling are reused.

## 12. Testing

Unit tests (no DB, mirroring current `tui` tests):
- `dashboard.toml` parse + serialize **round-trip**; the **built-in default parses
  and validates**.
- **Layout solver**: spans flow into the expected rows/`Rect`s; overflow wraps.
- Each panel's **`data_requests`** builds requests that pass `gauge_query::validate`
  under representative window/filters/pins (incl. the `stat` 3-request fan-out and
  numeric-attr resolution).
- **Filter merge**: global + per-panel pins compose correctly; **privacy guard** —
  `install_id`/`session_id` cannot be parsed into a filter field.
- **Delta math**: current vs previous window selection and ▲▼ sign.
- **Theme parse**: named, `ansi`, and custom-palette tables.
- Optional: `insta` snapshot tests over a `ratatui` `TestBackend` buffer for a couple
  of representative panels.

## 13. Migration from the current TUI

- `Page` → `Mode{Dashboard,Explore}`; `Overview`/`Apps` render paths are removed,
  their data needs covered by panels (`timeseries`, `stat`, `top_n`, `breakdown`,
  `apps_table`).
- `tui/ui.rs` render fns are split into the relevant `panels/*.rs`; `SERIES_COLORS`
  → `theme.rs`.
- `tui/data.rs` `fetch()`/`Snapshot` → the request-collection + `ResultMap` model;
  the histogram probe/`derive_edges` helpers move with the `histogram` panel.
- The Explore picker (`EXPLORE_MEASURES`/`EXPLORE_DIMENSIONS`, `explore_request`)
  is retained for the `Explore` mode, re-themed.
- `gauge tui` entry, the 30s poll, mpsc/`tokio::select!` loop, stale banner, and the
  `api.rs` re-login are preserved.

## 14. Open questions

None blocking. Future iterations (explicitly out of scope here): an absolute
date-range picker, saving/sharing presets by name beyond the single file, and
mouse interaction.
