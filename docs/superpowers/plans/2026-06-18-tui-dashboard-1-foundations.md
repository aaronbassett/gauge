# TUI Dashboard Redesign — Plan 1: Foundations

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the three pure, fully unit-tested foundation modules for the configurable dashboard — the theme system, the `dashboard.toml` config model (with the shipped default), and the span-based layout solver — without yet changing the running TUI.

**Architecture:** Three standalone modules under `crates/gauge/src/tui/` (`theme.rs`, `config.rs`, `layout.rs`) plus one helper in `paths.rs`. They are `pub` lib items, so they compile cleanly (no dead-code warnings under `clippy -D warnings`) before the run-loop wires them in (Plan 3). Each module is deterministic and tested in isolation; no async, no network, no rendering.

**Tech Stack:** Rust 2024, `ratatui 0.29` (`Color`, `BorderType`, `Rect`), `serde` + `toml` (already workspace deps), `gauge-query` types (`Filter`), `tempfile` (dev-dep) for filesystem tests.

**Plan series:** This is Plan 1 of 5. Later plans: 2 = panels & data layer, 3 = dashboard integration, 4 = filtering, 5 = live menu & persistence. This plan adds modules but does **not** alter `app.rs`/`run.rs`/`ui.rs` — `gauge tui` behaves exactly as before after Plan 1.

---

## File Structure

| File | Responsibility |
|---|---|
| `crates/gauge/src/tui/theme.rs` | **Create.** `Palette`, `Theme`, `BorderStyle`, `MeterStyle`; `parse_hex_color`; `builtin_palette(name)`. Pure colour data + parsing. |
| `crates/gauge/src/tui/config.rs` | **Create.** Serde model of `dashboard.toml` (`DashboardConfig`, `ThemeConfig`, `PaletteConfig`, `Borders`, `Meters`, `Preset`, `PanelSpec`, `Height`); `default_builtin()`; `resolve_theme()`; `validate()`; `load_from`/`save_to` (atomic) + `load`/`save`. |
| `crates/gauge/src/tui/layout.rs` | **Create.** `Cell`; `partition_rows`; `solve(area, cells) -> Vec<Rect>`. 12-column grid solver. |
| `crates/gauge/src/tui/mod.rs` | **Modify.** Register `pub mod theme; pub mod config; pub mod layout;`. |
| `crates/gauge/src/paths.rs` | **Modify.** Add `dashboard_path()`. |

`crate::tui::config` (dashboard) and `crate::config` (the existing `ClientConfig`) are different module paths — no collision.

---

## Task 1: Theme module — types + hex parsing

**Files:**
- Create: `crates/gauge/src/tui/theme.rs`
- Modify: `crates/gauge/src/tui/mod.rs`

- [ ] **Step 1: Create the module with types and a stub, and register it**

Create `crates/gauge/src/tui/theme.rs`:

```rust
//! Colour themes: built-in palettes, an ANSI mode that inherits the terminal's
//! 16 colours, and custom palettes parsed from `dashboard.toml`.

use ratatui::style::Color;
use ratatui::widgets::BorderType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderStyle {
    Rounded,
    Square,
}

impl BorderStyle {
    pub fn border_type(self) -> BorderType {
        match self {
            BorderStyle::Rounded => BorderType::Rounded,
            BorderStyle::Square => BorderType::Plain,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeterStyle {
    Gradient,
    Solid,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Palette {
    pub bg: Color,
    pub surface: Color,
    pub text: Color,
    pub muted: Color,
    pub up: Color,
    pub down: Color,
    /// At least two accent colours: per-series colours and gradient ramp endpoints.
    pub accents: Vec<Color>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub name: String,
    pub palette: Palette,
    pub border: BorderStyle,
    pub meter: MeterStyle,
}

/// Parse a `#rrggbb` string into `Color::Rgb`. Returns `None` for any other shape.
pub fn parse_hex_color(s: &str) -> Option<Color> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let r = u8::from_str_radix(&h[0..2], 16).ok()?;
    let g = u8::from_str_radix(&h[2..4], 16).ok()?;
    let b = u8::from_str_radix(&h[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}
```

Add to `crates/gauge/src/tui/mod.rs` (which currently reads `pub mod app; pub mod data; pub mod run; pub mod ui;`):

```rust
pub mod app;
pub mod config;
pub mod data;
pub mod layout;
pub mod run;
pub mod theme;
pub mod ui;
```

- [ ] **Step 2: Write the failing test**

Append to `crates/gauge/src/tui/theme.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_parses_and_rejects() {
        assert_eq!(parse_hex_color("#1a1b26"), Some(Color::Rgb(0x1a, 0x1b, 0x26)));
        assert_eq!(parse_hex_color("#FFFFFF"), Some(Color::Rgb(255, 255, 255)));
        assert_eq!(parse_hex_color("1a1b26"), None); // no '#'
        assert_eq!(parse_hex_color("#abc"), None); // wrong length
        assert_eq!(parse_hex_color("#gggggg"), None); // non-hex
    }

    #[test]
    fn border_style_maps() {
        assert_eq!(BorderStyle::Rounded.border_type(), BorderType::Rounded);
        assert_eq!(BorderStyle::Square.border_type(), BorderType::Plain);
    }
}
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p gauge-client tui::theme -- --nocapture`
Expected: PASS (`hex_parses_and_rejects`, `border_style_maps`). If the module fails to compile, fix the `mod.rs` registration before proceeding.

- [ ] **Step 4: Commit**

```bash
git add crates/gauge/src/tui/theme.rs crates/gauge/src/tui/mod.rs
git commit -m "feat(tui): theme types + hex colour parsing"
```

---

## Task 2: Built-in palettes

**Files:**
- Modify: `crates/gauge/src/tui/theme.rs`

- [ ] **Step 1: Write the failing test**

Add these tests inside the existing `mod tests` in `theme.rs`:

```rust
    #[test]
    fn tokyo_night_is_the_default_and_correct() {
        let p = builtin_palette("tokyo-night").expect("tokyo-night exists");
        assert_eq!(p.bg, Color::Rgb(0x1a, 0x1b, 0x26));
        assert_eq!(p.down, Color::Rgb(0xf7, 0x76, 0x8e));
        assert!(p.accents.len() >= 2, "need >=2 accents for gradients");
    }

    #[test]
    fn all_named_builtins_resolve() {
        for name in ["tokyo-night", "catppuccin-mocha", "gruvbox-dark", "nord", "ansi"] {
            let p = builtin_palette(name).unwrap_or_else(|| panic!("{name} missing"));
            assert!(p.accents.len() >= 2, "{name} needs >=2 accents");
        }
    }

    #[test]
    fn ansi_inherits_terminal_background() {
        let p = builtin_palette("ansi").unwrap();
        assert_eq!(p.bg, Color::Reset, "ANSI must not override terminal bg");
    }

    #[test]
    fn unknown_palette_is_none() {
        assert!(builtin_palette("not-a-theme").is_none());
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p gauge-client tui::theme -- --nocapture`
Expected: FAIL to compile — `builtin_palette` not found.

- [ ] **Step 3: Implement `builtin_palette`**

Add to `crates/gauge/src/tui/theme.rs` (before the `#[cfg(test)]` block):

```rust
/// Return a built-in palette by name, or `None` for an unknown name.
/// Names: `tokyo-night` (default), `catppuccin-mocha`, `gruvbox-dark`, `nord`, `ansi`.
pub fn builtin_palette(name: &str) -> Option<Palette> {
    let rgb = Color::Rgb;
    Some(match name {
        "tokyo-night" => Palette {
            bg: rgb(0x1a, 0x1b, 0x26),
            surface: rgb(0x2a, 0x2e, 0x44),
            text: rgb(0xc0, 0xca, 0xf5),
            muted: rgb(0x56, 0x5f, 0x89),
            up: rgb(0x9e, 0xce, 0x6a),
            down: rgb(0xf7, 0x76, 0x8e),
            accents: vec![
                rgb(0x7a, 0xa2, 0xf7),
                rgb(0x7d, 0xcf, 0xff),
                rgb(0xbb, 0x9a, 0xf7),
                rgb(0x9e, 0xce, 0x6a),
                rgb(0xe0, 0xaf, 0x68),
                rgb(0xf7, 0x76, 0x8e),
            ],
        },
        "catppuccin-mocha" => Palette {
            bg: rgb(0x1e, 0x1e, 0x2e),
            surface: rgb(0x31, 0x32, 0x44),
            text: rgb(0xcd, 0xd6, 0xf4),
            muted: rgb(0x6c, 0x70, 0x86),
            up: rgb(0xa6, 0xe3, 0xa1),
            down: rgb(0xf3, 0x8b, 0xa8),
            accents: vec![
                rgb(0x89, 0xb4, 0xfa),
                rgb(0x74, 0xc7, 0xec),
                rgb(0xcb, 0xa6, 0xf7),
                rgb(0xa6, 0xe3, 0xa1),
                rgb(0xf9, 0xe2, 0xaf),
                rgb(0xf3, 0x8b, 0xa8),
                rgb(0x94, 0xe2, 0xd5),
            ],
        },
        "gruvbox-dark" => Palette {
            bg: rgb(0x28, 0x28, 0x28),
            surface: rgb(0x3c, 0x38, 0x36),
            text: rgb(0xeb, 0xdb, 0xb2),
            muted: rgb(0x92, 0x83, 0x74),
            up: rgb(0xb8, 0xbb, 0x26),
            down: rgb(0xfb, 0x49, 0x34),
            accents: vec![
                rgb(0x83, 0xa5, 0x98),
                rgb(0xfa, 0xbd, 0x2f),
                rgb(0xfe, 0x80, 0x19),
                rgb(0x8e, 0xc0, 0x7c),
                rgb(0xd3, 0x86, 0x9b),
                rgb(0xb8, 0xbb, 0x26),
            ],
        },
        "nord" => Palette {
            bg: rgb(0x2e, 0x34, 0x40),
            surface: rgb(0x3b, 0x42, 0x52),
            text: rgb(0xe5, 0xe9, 0xf0),
            muted: rgb(0x4c, 0x56, 0x6a),
            up: rgb(0xa3, 0xbe, 0x8c),
            down: rgb(0xbf, 0x61, 0x6a),
            accents: vec![
                rgb(0x88, 0xc0, 0xd0),
                rgb(0x81, 0xa1, 0xc1),
                rgb(0xb4, 0x8e, 0xad),
                rgb(0xa3, 0xbe, 0x8c),
                rgb(0xeb, 0xcb, 0x8b),
                rgb(0x5e, 0x81, 0xac),
            ],
        },
        "ansi" => Palette {
            // Inherit the terminal's own colours where possible.
            bg: Color::Reset,
            surface: Color::DarkGray,
            text: Color::Reset,
            muted: Color::DarkGray,
            up: Color::Green,
            down: Color::Red,
            accents: vec![
                Color::Blue,
                Color::Cyan,
                Color::Magenta,
                Color::Green,
                Color::Yellow,
                Color::LightBlue,
            ],
        },
        _ => return None,
    })
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p gauge-client tui::theme -- --nocapture`
Expected: PASS (all theme tests).

- [ ] **Step 5: Commit**

```bash
git add crates/gauge/src/tui/theme.rs
git commit -m "feat(tui): built-in theme palettes (tokyo-night default, +4)"
```

---

## Task 3: `dashboard_path()` helper

**Files:**
- Modify: `crates/gauge/src/paths.rs`

- [ ] **Step 1: Write the failing test**

Append to `crates/gauge/src/paths.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashboard_path_sits_next_to_config() {
        // GAUGE_CONFIG_DIR takes precedence and is read synchronously here.
        unsafe { std::env::set_var("GAUGE_CONFIG_DIR", "/tmp/gauge-test-cfg") };
        let p = dashboard_path().unwrap();
        assert!(p.ends_with("dashboard.toml"));
        assert_eq!(p.parent().unwrap(), config_path().unwrap().parent().unwrap());
        unsafe { std::env::remove_var("GAUGE_CONFIG_DIR") };
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p gauge-client paths::tests::dashboard_path_sits_next_to_config -- --nocapture`
Expected: FAIL to compile — `dashboard_path` not found.

- [ ] **Step 3: Implement `dashboard_path`**

Add to `crates/gauge/src/paths.rs` after `config_path`:

```rust
pub fn dashboard_path() -> Result<PathBuf, ClientError> {
    Ok(config_dir()?.join("dashboard.toml"))
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p gauge-client paths::tests::dashboard_path_sits_next_to_config -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/gauge/src/paths.rs
git commit -m "feat(client): paths::dashboard_path for dashboard.toml"
```

---

## Task 4: Dashboard config types + TOML parse

**Files:**
- Create: `crates/gauge/src/tui/config.rs`

This task defines the serde model and proves a representative `dashboard.toml` parses. `mod config;` was already registered in Task 1.

- [ ] **Step 1: Create the config module**

Create `crates/gauge/src/tui/config.rs`:

```rust
//! The `dashboard.toml` model: theme, presets, and panel specs, plus the built-in
//! default config, theme resolution, validation, and atomic load/save.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::tui::theme::{self, BorderStyle, MeterStyle, Theme};

fn default_theme_name() -> String {
    "tokyo-night".to_string()
}
fn default_active_preset() -> String {
    "default".to_string()
}
fn default_span() -> u16 {
    12
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardConfig {
    // FIELD ORDER MATTERS for serialization: `toml` emits fields in declaration
    // order and errors (`ValueAfterTable`) if a root scalar follows a table. Keep
    // the scalar `active_preset` FIRST, before the `theme` table and `preset` array.
    #[serde(default = "default_active_preset")]
    pub active_preset: String,
    #[serde(default)]
    pub theme: ThemeConfig,
    /// `[[preset]]` tables.
    #[serde(default, rename = "preset")]
    pub presets: Vec<Preset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeConfig {
    #[serde(default = "default_theme_name")]
    pub name: String,
    #[serde(default)]
    pub borders: Borders,
    #[serde(default)]
    pub meters: Meters,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub palette: Option<PaletteConfig>,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            name: default_theme_name(),
            borders: Borders::default(),
            meters: Meters::default(),
            palette: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Borders {
    #[default]
    Rounded,
    Square,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Meters {
    #[default]
    Gradient,
    Solid,
}

impl From<Borders> for BorderStyle {
    fn from(b: Borders) -> Self {
        match b {
            Borders::Rounded => BorderStyle::Rounded,
            Borders::Square => BorderStyle::Square,
        }
    }
}
impl From<Meters> for MeterStyle {
    fn from(m: Meters) -> Self {
        match m {
            Meters::Gradient => MeterStyle::Gradient,
            Meters::Solid => MeterStyle::Solid,
        }
    }
}

/// Optional overrides for a custom palette. Any field left `None` keeps the base
/// palette's value; invalid hex strings are ignored (kept as base).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PaletteConfig {
    pub bg: Option<String>,
    pub surface: Option<String>,
    pub text: Option<String>,
    pub muted: Option<String>,
    pub up: Option<String>,
    pub down: Option<String>,
    #[serde(default)]
    pub accents: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    #[serde(default, rename = "panel")]
    pub panels: Vec<PanelSpec>,
}

/// A single panel. `kind` selects the renderer (Plan 2); the option fields below are
/// read per-kind and ignored when not relevant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelSpec {
    pub kind: String,
    #[serde(default = "default_span")]
    pub span: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<Height>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    // Per-kind options (all optional):
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric: Option<String>, // stat
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub metrics: Vec<String>, // timeseries
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_by: Option<String>, // timeseries
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>, // top_n / breakdown
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measure: Option<String>, // top_n
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>, // top_n
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attr: Option<String>, // numeric_stats / histogram

    /// Static per-panel filter pins, merged with the global filter bar at query time.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<gauge_query::Filter>,
}

/// A panel row height: a fixed number of terminal rows, or the keyword `"fill"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Height {
    Rows(u16),
    Keyword(String),
}

impl Height {
    /// Fixed row count, or `None` for `"fill"`.
    pub fn rows(&self) -> Option<u16> {
        match self {
            Height::Rows(n) => Some(*n),
            Height::Keyword(_) => None,
        }
    }
}
```

- [ ] **Step 2: Write the failing test**

Append to `crates/gauge/src/tui/config.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
active_preset = "default"

[theme]
name = "nord"
borders = "square"
meters = "solid"

[[preset]]
name = "default"

  [[preset.panel]]
  kind = "timeseries"
  metrics = ["events", "unique_installs"]
  span = 12
  height = 8

  [[preset.panel]]
  kind = "top_n"
  field = "event_name"
  measure = "count"
  limit = 5
  span = 6
  height = "fill"
  filters = [ { field = "app", op = "eq", value = "tome" } ]
"#;

    #[test]
    fn parses_a_representative_dashboard_toml() {
        let cfg: DashboardConfig = toml::from_str(SAMPLE).unwrap();
        assert_eq!(cfg.theme.name, "nord");
        assert_eq!(cfg.theme.borders, Borders::Square);
        assert_eq!(cfg.theme.meters, Meters::Solid);
        assert_eq!(cfg.presets.len(), 1);
        let preset = &cfg.presets[0];
        assert_eq!(preset.name, "default");
        assert_eq!(preset.panels.len(), 2);

        let ts = &preset.panels[0];
        assert_eq!(ts.kind, "timeseries");
        assert_eq!(ts.metrics, vec!["events", "unique_installs"]);
        assert_eq!(ts.height, Some(Height::Rows(8)));

        let top = &preset.panels[1];
        assert_eq!(top.kind, "top_n");
        assert_eq!(top.field.as_deref(), Some("event_name"));
        assert_eq!(top.limit, Some(5));
        assert_eq!(top.height.as_ref().unwrap().rows(), None); // "fill"
        assert_eq!(top.filters.len(), 1);
    }

    #[test]
    fn missing_optional_sections_use_defaults() {
        let cfg: DashboardConfig = toml::from_str("").unwrap();
        assert_eq!(cfg.theme.name, "tokyo-night");
        assert_eq!(cfg.theme.borders, Borders::Rounded);
        assert_eq!(cfg.active_preset, "default");
        assert!(cfg.presets.is_empty());
    }
}
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p gauge-client tui::config -- --nocapture`
Expected: PASS (`parses_a_representative_dashboard_toml`, `missing_optional_sections_use_defaults`).

> Note: if `toml` rejects the untagged `Height` (number vs string), confirm the field uses `#[serde(untagged)]` exactly as written — `toml` deserializes integers to `Height::Rows` and strings to `Height::Keyword` via untagged matching.

- [ ] **Step 4: Commit**

```bash
git add crates/gauge/src/tui/config.rs
git commit -m "feat(tui): dashboard.toml serde model"
```

---

## Task 5: Built-in default config + validation + preset lookup

**Files:**
- Modify: `crates/gauge/src/tui/config.rs`

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `config.rs`:

```rust
    #[test]
    fn builtin_default_matches_the_approved_layout() {
        let cfg = default_builtin();
        assert_eq!(cfg.theme.name, "tokyo-night");
        let preset = cfg.active_preset().expect("default preset resolves");
        assert_eq!(preset.name, "default");

        let kinds: Vec<&str> = preset.panels.iter().map(|p| p.kind.as_str()).collect();
        assert_eq!(
            kinds,
            vec![
                "timeseries",
                "stat", "stat", "stat", "stat",
                "top_n",
                "numeric_stats",
                "breakdown", "breakdown", "breakdown",
            ]
        );
        // Row 2 is four span-3 stat tiles.
        assert!(preset.panels[1..5].iter().all(|p| p.span == 3));
        // The default validates.
        cfg.validate().expect("built-in default must validate");
    }

    #[test]
    fn validate_rejects_bad_span_and_missing_preset() {
        let mut cfg = default_builtin();
        cfg.presets[0].panels[0].span = 13;
        assert!(cfg.validate().is_err(), "span > 12 must fail");

        let mut cfg2 = default_builtin();
        cfg2.active_preset = "nope".into();
        assert!(cfg2.validate().is_err(), "unknown active_preset must fail");
    }

    #[test]
    fn default_round_trips_through_toml() {
        let cfg = default_builtin();
        let s = toml::to_string(&cfg).unwrap();
        let back: DashboardConfig = toml::from_str(&s).unwrap();
        assert_eq!(back.active_preset, cfg.active_preset);
        assert_eq!(back.presets[0].panels.len(), cfg.presets[0].panels.len());
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p gauge-client tui::config -- --nocapture`
Expected: FAIL to compile — `default_builtin`, `active_preset`, `validate` not found.

- [ ] **Step 3: Implement the default, lookup, and validation**

Add to `crates/gauge/src/tui/config.rs` (an `impl DashboardConfig` block, before the test module):

```rust
impl DashboardConfig {
    /// The shipped default dashboard (the approved layout). Constructed in code so
    /// the TUI works with no `dashboard.toml` present.
    pub fn default_builtin() -> DashboardConfig {
        fn stat(metric: &str, span: u16) -> PanelSpec {
            PanelSpec {
                kind: "stat".into(),
                span,
                height: Some(Height::Rows(4)),
                title: None,
                metric: Some(metric.into()),
                metrics: vec![],
                group_by: None,
                field: None,
                measure: None,
                limit: None,
                attr: None,
                filters: vec![],
            }
        }
        fn breakdown(field: &str, title: &str, span: u16) -> PanelSpec {
            PanelSpec {
                kind: "breakdown".into(),
                span,
                height: Some(Height::Rows(6)),
                title: Some(title.into()),
                metric: None,
                metrics: vec![],
                group_by: None,
                field: Some(field.into()),
                measure: None,
                limit: None,
                attr: None,
                filters: vec![],
            }
        }

        let timeseries = PanelSpec {
            kind: "timeseries".into(),
            span: 12,
            height: Some(Height::Rows(9)),
            title: Some("Activity".into()),
            metric: None,
            metrics: vec![
                "events".into(),
                "unique_installs".into(),
                "unique_sessions".into(),
            ],
            group_by: None,
            field: None,
            measure: None,
            limit: None,
            attr: None,
            filters: vec![],
        };
        let top_events = PanelSpec {
            kind: "top_n".into(),
            span: 6,
            height: Some(Height::Keyword("fill".into())),
            title: Some("Top events".into()),
            metric: None,
            metrics: vec![],
            group_by: None,
            field: Some("event_name".into()),
            measure: Some("count".into()),
            limit: Some(5),
            attr: None,
            filters: vec![],
        };
        let latency = PanelSpec {
            kind: "numeric_stats".into(),
            span: 6,
            height: Some(Height::Keyword("fill".into())),
            title: Some("Latency distribution".into()),
            metric: None,
            metrics: vec![],
            group_by: None,
            field: None,
            measure: None,
            limit: None,
            attr: None, // auto-resolve to first numeric attr in meta (Plan 2)
            filters: vec![],
        };

        let panels = vec![
            timeseries,
            stat("events", 3),
            stat("unique_installs", 3),
            stat("unique_sessions", 3),
            stat("p95", 3), // aggregate stat; attr auto-resolved in Plan 2
            top_events,
            latency,
            breakdown("os", "OS", 4),
            breakdown("arch", "Arch", 4),
            breakdown("app_version", "Versions", 4),
        ];

        DashboardConfig {
            theme: ThemeConfig::default(),
            active_preset: "default".into(),
            presets: vec![Preset {
                name: "default".into(),
                panels,
            }],
        }
    }

    /// The preset named by `active_preset`, falling back to the first preset.
    pub fn active_preset(&self) -> Option<&Preset> {
        self.presets
            .iter()
            .find(|p| p.name == self.active_preset)
            .or_else(|| self.presets.first())
    }

    /// Structural checks that don't need the panel registry (Plan 2 validates `kind`).
    pub fn validate(&self) -> Result<(), String> {
        if self.presets.is_empty() {
            return Err("no presets defined".into());
        }
        if !self.presets.iter().any(|p| p.name == self.active_preset) {
            return Err(format!("active_preset `{}` not found", self.active_preset));
        }
        for preset in &self.presets {
            for panel in &preset.panels {
                if !(1..=12).contains(&panel.span) {
                    return Err(format!(
                        "panel `{}` in preset `{}` has span {} (must be 1..=12)",
                        panel.kind, preset.name, panel.span
                    ));
                }
            }
        }
        Ok(())
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p gauge-client tui::config -- --nocapture`
Expected: PASS (all config tests so far).

- [ ] **Step 5: Commit**

```bash
git add crates/gauge/src/tui/config.rs
git commit -m "feat(tui): built-in default dashboard + validation + preset lookup"
```

---

## Task 6: Theme resolution from config

**Files:**
- Modify: `crates/gauge/src/tui/config.rs`

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `config.rs`:

```rust
    #[test]
    fn resolve_theme_uses_builtin_and_style_flags() {
        use crate::tui::theme::{BorderStyle, MeterStyle};
        let mut cfg = default_builtin();
        cfg.theme.name = "nord".into();
        cfg.theme.borders = Borders::Square;
        cfg.theme.meters = Meters::Solid;
        let theme = cfg.resolve_theme();
        assert_eq!(theme.name, "nord");
        assert_eq!(theme.border, BorderStyle::Square);
        assert_eq!(theme.meter, MeterStyle::Solid);
        assert_eq!(theme.palette.bg, ratatui::style::Color::Rgb(0x2e, 0x34, 0x40));
    }

    #[test]
    fn resolve_theme_falls_back_to_tokyo_night_for_unknown_name() {
        let mut cfg = default_builtin();
        cfg.theme.name = "totally-unknown".into();
        let theme = cfg.resolve_theme();
        // palette falls back to tokyo-night even though the stored name is preserved
        assert_eq!(theme.palette.down, ratatui::style::Color::Rgb(0xf7, 0x76, 0x8e));
    }

    #[test]
    fn resolve_theme_applies_custom_palette_overrides() {
        let mut cfg = default_builtin();
        cfg.theme.palette = Some(PaletteConfig {
            bg: Some("#000000".into()),
            text: Some("not-a-colour".into()), // ignored, keeps base
            ..Default::default()
        });
        let theme = cfg.resolve_theme();
        assert_eq!(theme.palette.bg, ratatui::style::Color::Rgb(0, 0, 0));
        // base tokyo-night text retained because the override was invalid
        assert_eq!(theme.palette.text, ratatui::style::Color::Rgb(0xc0, 0xca, 0xf5));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p gauge-client tui::config -- --nocapture`
Expected: FAIL to compile — `resolve_theme` not found.

- [ ] **Step 3: Implement `resolve_theme` and `PaletteConfig::apply_to`**

Add to `crates/gauge/src/tui/config.rs`:

```rust
impl PaletteConfig {
    /// Apply any valid hex overrides on top of a base palette.
    fn apply_to(&self, mut base: theme::Palette) -> theme::Palette {
        fn set(target: &mut ratatui::style::Color, hex: &Option<String>) {
            if let Some(s) = hex {
                if let Some(c) = theme::parse_hex_color(s) {
                    *target = c;
                }
            }
        }
        set(&mut base.bg, &self.bg);
        set(&mut base.surface, &self.surface);
        set(&mut base.text, &self.text);
        set(&mut base.muted, &self.muted);
        set(&mut base.up, &self.up);
        set(&mut base.down, &self.down);
        let parsed: Vec<_> = self
            .accents
            .iter()
            .filter_map(|s| theme::parse_hex_color(s))
            .collect();
        if parsed.len() >= 2 {
            base.accents = parsed;
        }
        base
    }
}

impl DashboardConfig {
    /// Build the runtime `Theme` from the config: a built-in palette (falling back to
    /// tokyo-night for an unknown name), optionally overridden by `[theme.palette]`,
    /// plus the border/meter style flags.
    pub fn resolve_theme(&self) -> Theme {
        let base = theme::builtin_palette(&self.theme.name)
            .or_else(|| theme::builtin_palette("tokyo-night"))
            .expect("tokyo-night palette always exists");
        let palette = match &self.theme.palette {
            Some(p) => p.apply_to(base),
            None => base,
        };
        Theme {
            name: self.theme.name.clone(),
            palette,
            border: self.theme.borders.into(),
            meter: self.theme.meters.into(),
        }
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p gauge-client tui::config -- --nocapture`
Expected: PASS (all config tests).

- [ ] **Step 5: Commit**

```bash
git add crates/gauge/src/tui/config.rs
git commit -m "feat(tui): resolve Theme from dashboard config + custom palette overrides"
```

---

## Task 7: Atomic load/save

**Files:**
- Modify: `crates/gauge/src/tui/config.rs`

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `config.rs`:

```rust
    #[test]
    fn save_to_then_load_from_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dashboard.toml");

        let cfg = default_builtin();
        cfg.save_to(&path).unwrap();
        assert!(path.exists());

        let (loaded, err) = load_from(&path);
        assert!(err.is_none(), "valid file should not report an error");
        assert_eq!(loaded.active_preset, cfg.active_preset);
        assert_eq!(
            loaded.presets[0].panels.len(),
            cfg.presets[0].panels.len()
        );
    }

    #[test]
    fn load_from_missing_file_yields_default_without_error() {
        let (cfg, err) = load_from(Path::new("/tmp/gauge-does-not-exist-xyz.toml"));
        assert!(err.is_none(), "missing file is not an error");
        assert_eq!(cfg.active_preset, "default"); // the built-in default
    }

    #[test]
    fn load_from_invalid_file_yields_default_with_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dashboard.toml");
        std::fs::write(&path, "this = is = not = toml").unwrap();

        let (cfg, err) = load_from(&path);
        assert!(err.is_some(), "invalid toml must surface an error string");
        assert_eq!(cfg.active_preset, "default"); // fell back to default
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p gauge-client tui::config -- --nocapture`
Expected: FAIL to compile — `save_to`, `load_from` not found.

- [ ] **Step 3: Implement load/save**

Add to `crates/gauge/src/tui/config.rs`. First extend the imports at the top of the file:

```rust
use crate::error::ClientError;
use crate::paths;
```

Then add:

```rust
impl DashboardConfig {
    /// Write atomically: serialize to a sibling `*.tmp` file, then rename over the
    /// target so a crash mid-write can never leave a truncated config.
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let body = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, body)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Persist to the resolved `dashboard.toml` path.
    pub fn save(&self) -> Result<(), ClientError> {
        let path = paths::dashboard_path()?;
        self.save_to(&path)?;
        Ok(())
    }
}

/// Load from an explicit path. Missing file → built-in default, no error. Invalid
/// file → built-in default plus an error string describing the problem.
pub fn load_from(path: &Path) -> (DashboardConfig, Option<String>) {
    match std::fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            (DashboardConfig::default_builtin(), None)
        }
        Err(e) => (
            DashboardConfig::default_builtin(),
            Some(format!("could not read {}: {e}", path.display())),
        ),
        Ok(raw) => match toml::from_str::<DashboardConfig>(&raw) {
            Ok(cfg) => match cfg.validate() {
                Ok(()) => (cfg, None),
                Err(msg) => (
                    DashboardConfig::default_builtin(),
                    Some(format!("invalid dashboard config: {msg}")),
                ),
            },
            Err(e) => (
                DashboardConfig::default_builtin(),
                Some(format!("could not parse {}: {e}", path.display())),
            ),
        },
    }
}

/// Load from the resolved `dashboard.toml` path.
pub fn load() -> (DashboardConfig, Option<String>) {
    match paths::dashboard_path() {
        Ok(path) => load_from(&path),
        Err(e) => (DashboardConfig::default_builtin(), Some(e.to_string())),
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p gauge-client tui::config -- --nocapture`
Expected: PASS (all config tests).

- [ ] **Step 5: Commit**

```bash
git add crates/gauge/src/tui/config.rs
git commit -m "feat(tui): atomic load/save for dashboard.toml with default fallback"
```

---

## Task 8: Layout solver — row partitioning

**Files:**
- Create: `crates/gauge/src/tui/layout.rs`

`mod layout;` was registered in Task 1.

- [ ] **Step 1: Create the module with `Cell` + `partition_rows`**

Create `crates/gauge/src/tui/layout.rs`:

```rust
//! The dashboard layout solver: flow panels left-to-right across a 12-column grid,
//! wrapping to a new row on overflow, then assign each panel a `Rect`.

use ratatui::layout::Rect;

use crate::tui::config::{Height, PanelSpec};

/// A panel's grid footprint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    /// Grid columns, clamped to 1..=12.
    pub span: u16,
    /// Fixed terminal rows, or `None` to share leftover vertical space.
    pub height: Option<u16>,
}

impl Cell {
    pub fn from_spec(p: &PanelSpec) -> Cell {
        Cell {
            span: p.span.clamp(1, 12),
            height: p.height.as_ref().and_then(Height::rows),
        }
    }
}

/// Group cell indices into rows; a new row begins when the next cell would push the
/// running column total past 12.
pub fn partition_rows(cells: &[Cell]) -> Vec<Vec<usize>> {
    let mut rows: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    let mut used = 0u16;
    for (i, c) in cells.iter().enumerate() {
        let span = c.span.clamp(1, 12);
        if used + span > 12 && !cur.is_empty() {
            rows.push(std::mem::take(&mut cur));
            used = 0;
        }
        cur.push(i);
        used += span;
    }
    if !cur.is_empty() {
        rows.push(cur);
    }
    rows
}
```

- [ ] **Step 2: Write the failing test**

Append to `crates/gauge/src/tui/layout.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn cells(spans: &[u16]) -> Vec<Cell> {
        spans.iter().map(|&span| Cell { span, height: None }).collect()
    }

    #[test]
    fn partitions_the_default_layout_into_four_rows() {
        // timeseries(12), 4x stat(3), top_n(6)+numeric_stats(6), 3x breakdown(4)
        let c = cells(&[12, 3, 3, 3, 3, 6, 6, 4, 4, 4]);
        let rows = partition_rows(&c);
        assert_eq!(
            rows,
            vec![
                vec![0],
                vec![1, 2, 3, 4],
                vec![5, 6],
                vec![7, 8, 9],
            ]
        );
    }

    #[test]
    fn a_single_overfull_cell_still_gets_its_own_row() {
        let c = cells(&[12, 12]);
        assert_eq!(partition_rows(&c), vec![vec![0], vec![1]]);
    }
}
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p gauge-client tui::layout -- --nocapture`
Expected: PASS (`partitions_the_default_layout_into_four_rows`, `a_single_overfull_cell_still_gets_its_own_row`).

- [ ] **Step 4: Commit**

```bash
git add crates/gauge/src/tui/layout.rs
git commit -m "feat(tui): layout row partitioning over a 12-column grid"
```

---

## Task 9: Layout solver — Rect assignment

**Files:**
- Modify: `crates/gauge/src/tui/layout.rs`

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `layout.rs`:

```rust
    #[test]
    fn solve_places_a_full_row_across_the_width() {
        let area = Rect { x: 0, y: 0, width: 120, height: 40 };
        let c = cells(&[12]); // one full-width flexible row
        let rects = solve(area, &c);
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0].x, 0);
        assert_eq!(rects[0].y, 0);
        assert_eq!(rects[0].width, 120);
        assert_eq!(rects[0].height, 40); // only flexible row → takes all height
    }

    #[test]
    fn solve_splits_a_row_by_span_and_stacks_rows() {
        // width 120 → col_unit 10. Row A: 4x span-3 fixed height 4. Row B: span-12 fill.
        let area = Rect { x: 0, y: 0, width: 120, height: 24 };
        let c = vec![
            Cell { span: 3, height: Some(4) },
            Cell { span: 3, height: Some(4) },
            Cell { span: 3, height: Some(4) },
            Cell { span: 3, height: Some(4) },
            Cell { span: 12, height: None },
        ];
        let rects = solve(area, &c);
        // Row A: y=0, each height 4, widths 30 each, x at 0/30/60/90.
        for (i, x) in [0u16, 30, 60, 90].iter().enumerate() {
            assert_eq!(rects[i].y, 0);
            assert_eq!(rects[i].height, 4);
            assert_eq!(rects[i].x, *x);
            assert_eq!(rects[i].width, 30);
        }
        // Row B: starts at y=4, gets remaining height 24-4=20, full width.
        assert_eq!(rects[4].y, 4);
        assert_eq!(rects[4].height, 20);
        assert_eq!(rects[4].width, 120);
    }

    #[test]
    fn solve_is_safe_for_zero_area() {
        let rects = solve(Rect::default(), &cells(&[6, 6]));
        assert_eq!(rects.len(), 2);
        assert!(rects.iter().all(|r| r.width == 0 && r.height == 0));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p gauge-client tui::layout -- --nocapture`
Expected: FAIL to compile — `solve` not found.

- [ ] **Step 3: Implement `solve`**

Add to `crates/gauge/src/tui/layout.rs` (before the test module):

```rust
/// Assign a `Rect` to each cell inside `area` over a 12-column grid.
///
/// Row heights: a row containing any fixed-height cell takes the max fixed height in
/// that row; the remaining vertical space is split evenly among flexible rows (each
/// at least 1 line, with any rounding remainder handed to the earliest flexible rows).
/// Within a row, each cell's width is `span * (area.width / 12)`; the last cell in a
/// row absorbs the rounding remainder up to its grid edge.
pub fn solve(area: Rect, cells: &[Cell]) -> Vec<Rect> {
    let mut out = vec![Rect::default(); cells.len()];
    if cells.is_empty() || area.width == 0 || area.height == 0 {
        return out;
    }
    let rows = partition_rows(cells);

    // Per-row fixed height (max of fixed cells), or None if the row is fully flexible.
    let row_fixed: Vec<Option<u16>> = rows
        .iter()
        .map(|r| r.iter().filter_map(|&i| cells[i].height).max())
        .collect();
    let fixed_total: u16 = row_fixed.iter().flatten().sum();
    let flex_count = row_fixed.iter().filter(|h| h.is_none()).count() as u16;
    let remaining = area.height.saturating_sub(fixed_total);
    let flex_each = if flex_count > 0 {
        (remaining / flex_count).max(1)
    } else {
        0
    };
    let mut flex_rem = if flex_count > 0 {
        remaining.saturating_sub(flex_each * flex_count)
    } else {
        0
    };

    let col_unit = area.width / 12;
    let mut y = area.y;
    let bottom = area.y.saturating_add(area.height);

    for (ri, row) in rows.iter().enumerate() {
        let mut row_h = match row_fixed[ri] {
            Some(h) => h,
            None => {
                let mut h = flex_each;
                if flex_rem > 0 {
                    h += 1;
                    flex_rem -= 1;
                }
                h
            }
        };
        // Never run past the bottom of the area.
        row_h = row_h.min(bottom.saturating_sub(y));
        if row_h == 0 {
            break;
        }

        let mut x = area.x;
        let mut used_cols = 0u16;
        for (pos, &i) in row.iter().enumerate() {
            let span = cells[i].span.clamp(1, 12);
            let grid_edge = area.x + (used_cols + span).min(12) * col_unit;
            let width = if pos + 1 == row.len() {
                grid_edge.saturating_sub(x).max(1)
            } else {
                (span * col_unit).max(1)
            };
            out[i] = Rect {
                x,
                y,
                width,
                height: row_h,
            };
            x = x.saturating_add(width);
            used_cols += span;
        }

        y = y.saturating_add(row_h);
        if y >= bottom {
            break;
        }
    }
    out
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p gauge-client tui::layout -- --nocapture`
Expected: PASS (all layout tests).

- [ ] **Step 5: Commit**

```bash
git add crates/gauge/src/tui/layout.rs
git commit -m "feat(tui): 12-column layout solver (rect assignment)"
```

---

## Task 10: Foundation gate — full build, clippy, and test sweep

**Files:** none (verification only)

- [ ] **Step 1: Build the whole workspace**

Run: `cargo build -p gauge-client`
Expected: compiles with no errors.

- [ ] **Step 2: Run clippy with the CI's deny level**

Run: `cargo clippy -p gauge-client --all-targets -- -D warnings`
Expected: no warnings. (The new modules are `pub`, so unused items do not trigger dead-code warnings in the lib crate.) Fix any lint inline, then re-run.

- [ ] **Step 3: Run the full client test suite**

Run: `cargo test -p gauge-client`
Expected: all existing tests still pass, plus the new `tui::theme`, `tui::config`, `tui::layout`, and `paths` tests.

- [ ] **Step 4: Confirm the running TUI is unchanged**

Run: `cargo build -p gauge-client && echo "tui entrypoint intact"`
Expected: builds. `app.rs`/`run.rs`/`ui.rs` were not touched, so `gauge tui` behaves exactly as before — these foundations are not wired in until Plan 3.

- [ ] **Step 5: Commit (if clippy required any fixes)**

```bash
git add -A
git commit -m "chore(tui): foundation modules pass build + clippy -D warnings"
```

---

## Done criteria for Plan 1

- `theme.rs`, `tui/config.rs`, `layout.rs`, and `paths::dashboard_path` exist and are fully unit-tested.
- `DashboardConfig::default_builtin()` reproduces the approved layout and round-trips through TOML.
- `cargo clippy -p gauge-client --all-targets -- -D warnings` is clean.
- `gauge tui` still runs the old UI (no integration yet).

**Next:** Plan 2 (panels & data layer) consumes `PanelSpec` via a `Panel` trait + factory, builds each panel's `QueryRequest`s (validated against `gauge_query::validate`), and adds the concurrent dedup fetch.
