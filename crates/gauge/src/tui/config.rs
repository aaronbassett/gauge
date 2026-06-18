//! The `dashboard.toml` model: theme, presets, and panel specs, plus the built-in
//! default config, theme resolution, validation, and atomic load/save.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::ClientError;
use crate::paths;
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<f64>, // histogram bucket edges

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
                edges: vec![],
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
                edges: vec![],
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
            edges: vec![],
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
            edges: vec![],
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
            edges: vec![],
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

impl PaletteConfig {
    /// Apply any valid hex overrides on top of a base palette.
    fn apply_to(&self, mut base: theme::Palette) -> theme::Palette {
        fn set(target: &mut ratatui::style::Color, hex: &Option<String>) {
            if let Some(s) = hex
                && let Some(c) = theme::parse_hex_color(s)
            {
                *target = c;
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

    #[test]
    fn builtin_default_matches_the_approved_layout() {
        let cfg = DashboardConfig::default_builtin();
        assert_eq!(cfg.theme.name, "tokyo-night");
        let preset = cfg.active_preset().expect("default preset resolves");
        assert_eq!(preset.name, "default");

        let kinds: Vec<&str> = preset.panels.iter().map(|p| p.kind.as_str()).collect();
        assert_eq!(
            kinds,
            vec![
                "timeseries",
                "stat",
                "stat",
                "stat",
                "stat",
                "top_n",
                "numeric_stats",
                "breakdown",
                "breakdown",
                "breakdown",
            ]
        );
        // Row 2 is four span-3 stat tiles.
        assert!(preset.panels[1..5].iter().all(|p| p.span == 3));
        // The default validates.
        cfg.validate().expect("built-in default must validate");
    }

    #[test]
    fn validate_rejects_bad_span_and_missing_preset() {
        let mut cfg = DashboardConfig::default_builtin();
        cfg.presets[0].panels[0].span = 13;
        assert!(cfg.validate().is_err(), "span > 12 must fail");

        let mut cfg2 = DashboardConfig::default_builtin();
        cfg2.active_preset = "nope".into();
        assert!(cfg2.validate().is_err(), "unknown active_preset must fail");
    }

    #[test]
    fn default_round_trips_through_toml() {
        let cfg = DashboardConfig::default_builtin();
        let s = toml::to_string(&cfg).unwrap();
        let back: DashboardConfig = toml::from_str(&s).unwrap();
        assert_eq!(back.active_preset, cfg.active_preset);
        assert_eq!(back.presets[0].panels.len(), cfg.presets[0].panels.len());
    }

    #[test]
    fn resolve_theme_uses_builtin_and_style_flags() {
        use crate::tui::theme::{BorderStyle, MeterStyle};
        let mut cfg = DashboardConfig::default_builtin();
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
        let mut cfg = DashboardConfig::default_builtin();
        cfg.theme.name = "totally-unknown".into();
        let theme = cfg.resolve_theme();
        // palette falls back to tokyo-night even though the stored name is preserved
        assert_eq!(theme.palette.down, ratatui::style::Color::Rgb(0xf7, 0x76, 0x8e));
    }

    #[test]
    fn resolve_theme_applies_custom_palette_overrides() {
        let mut cfg = DashboardConfig::default_builtin();
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

    #[test]
    fn save_to_then_load_from_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dashboard.toml");

        let cfg = DashboardConfig::default_builtin();
        cfg.save_to(&path).unwrap();
        assert!(path.exists());

        let (loaded, err) = load_from(&path);
        assert!(err.is_none(), "valid file should not report an error");
        assert_eq!(loaded.active_preset, cfg.active_preset);
        assert_eq!(loaded.presets[0].panels.len(), cfg.presets[0].panels.len());
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

    #[test]
    fn panel_spec_parses_histogram_edges() {
        let toml = r#"
active_preset = "d"
[[preset]]
name = "d"
  [[preset.panel]]
  kind = "histogram"
  attr = "latency_ms"
  edges = [50, 200, 600]
"#;
        let cfg: DashboardConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.presets[0].panels[0].edges, vec![50.0, 200.0, 600.0]);
    }

    #[test]
    fn panel_spec_edges_default_empty() {
        let cfg = DashboardConfig::default_builtin();
        assert!(cfg.presets[0].panels[0].edges.is_empty());
    }
}
