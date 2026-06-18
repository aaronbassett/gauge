# TUI Dashboard Redesign — Plan 5: Live Menu & Persistence

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the live customization menu (`m`) — switch preset, cycle theme, toggle border/meter style, and show/hide panels — with every edit **persisted back to `dashboard.toml`** via the atomic write from Plan 1. Then update the README and remove the now-dead `data::{fetch,Snapshot}`.

**Architecture:** A second modal in `app.rs` (`MenuState`, focus over a flat list of rows: preset, theme, borders, meters, then one row per panel). Edits mutate the in-memory `DashboardConfig`, rebuild panels, and set a `config_dirty` flag; the run loop persists when dirty (`config.save()`), surfacing any write error in the existing banner. Panel show/hide uses a new `PanelSpec.hidden` field that `rebuild_panels` skips. In-menu panel add/remove/reorder is intentionally **out of scope** (config-file-only for now) — noted as future.

**Tech Stack:** Rust 2024, `ratatui 0.29`, `gauge-query`, `toml`.

**Plan series:** Plan 5 of 5 (final). Depends on Plans 1–4. After this, the full design is implemented.

---

## File Structure

| File | Responsibility |
|---|---|
| `crates/gauge/src/tui/config.rs` | **Modify.** Add `hidden: bool` to `PanelSpec`. |
| `crates/gauge/src/tui/app.rs` | **Modify.** `MenuState`; `App.menu` + `App.config_dirty`; `m` binding + dispatch; `menu_key`/`menu_adjust`/`menu_toggle`/`cycle_theme`/`cycle_preset_dir`/`active_preset_index`/`after_config_change`; `rebuild_panels` skips hidden. |
| `crates/gauge/src/tui/run.rs` | **Modify.** Persist when `config_dirty`. |
| `crates/gauge/src/tui/ui.rs` | **Modify.** `render_menu_overlay` + call from `render`; add `m` to status hints. |
| `crates/gauge/src/tui/data.rs` | **Modify.** Remove dead `fetch` + `Snapshot` (keep `base`, histogram helpers, `TimeWindow`). |
| `README.md` | **Modify.** Rewrite the `### TUI` subsection. |

---

## Task 1: `PanelSpec.hidden`

**Files:**
- Modify: `crates/gauge/src/tui/config.rs`

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `config.rs`:

```rust
    #[test]
    fn hidden_defaults_false_and_round_trips() {
        let toml = r#"
active_preset = "d"
[[preset]]
name = "d"
  [[preset.panel]]
  kind = "stat"
  metric = "events"
  hidden = true
"#;
        let cfg: DashboardConfig = toml::from_str(toml).unwrap();
        assert!(cfg.presets[0].panels[0].hidden);
        assert!(!default_builtin().presets[0].panels[0].hidden);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p gauge-client tui::config::tests::hidden_defaults_false_and_round_trips -- --nocapture`
Expected: FAIL to compile — no field `hidden`.

- [ ] **Step 3: Add the field**

In `PanelSpec` (after `edges`, before `filters`):

```rust
    #[serde(default)]
    pub hidden: bool,
```

Add `hidden: false,` to every `PanelSpec { .. }` literal in `default_builtin()` (the `stat`/`breakdown` helpers and the three named literals), alongside `edges: vec![],`.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p gauge-client tui::config -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/gauge/src/tui/config.rs
git commit -m "feat(tui): PanelSpec.hidden for show/hide"
```

---

## Task 2: Menu state machine in `app.rs`

**Files:**
- Modify: `crates/gauge/src/tui/app.rs`

- [ ] **Step 1: Add `MenuState`, fields, and the theme list**

Add near `FilterDraft`:

```rust
/// Live customization menu. `focus` indexes a flat list of rows:
/// 0=preset, 1=theme, 2=borders, 3=meters, then 4.. one row per panel.
#[derive(Debug, Clone)]
pub struct MenuState {
    pub focus: usize,
}

pub const BUILTIN_THEMES: &[&str] =
    &["tokyo-night", "catppuccin-mocha", "gruvbox-dark", "nord", "ansi"];
```

Add to `App`:

```rust
    pub menu: Option<MenuState>,
    pub config_dirty: bool,
```

…and initialise in `App::new`: `menu: None,` and `config_dirty: false,`.

Import the config enums needed for toggling — extend the `config` import:

```rust
use crate::tui::config::{self, Borders, DashboardConfig, Meters};
```

- [ ] **Step 2: Make `rebuild_panels` skip hidden panels**

In `rebuild_panels`, change the loop to skip hidden specs:

```rust
        for spec in &specs {
            if spec.hidden {
                continue;
            }
            match panels::build(spec) {
                Ok(p) => {
                    self.cells.push(Cell::from_spec(spec));
                    self.panels.push(p);
                }
                Err(e) => errs.push(format!("{}: {e}", spec.kind)),
            }
        }
```

- [ ] **Step 3: Add the menu methods**

Add inside `impl App`:

```rust
    fn menu_panel_count(&self) -> usize {
        self.config.active_preset().map(|p| p.panels.len()).unwrap_or(0)
    }

    fn menu_rows(&self) -> usize {
        4 + self.menu_panel_count()
    }

    fn active_preset_index(&self) -> Option<usize> {
        let name = &self.config.active_preset;
        self.config
            .presets
            .iter()
            .position(|p| p.name == *name)
            .or(if self.config.presets.is_empty() { None } else { Some(0) })
    }

    /// After any config mutation: rebuild panels, mark dirty (run loop persists), refresh.
    fn after_config_change(&mut self) {
        self.rebuild_panels();
        self.config_dirty = true;
        self.refresh_requested = true;
    }

    fn cycle_theme(&mut self, forward: bool) {
        let n = BUILTIN_THEMES.len();
        let cur = BUILTIN_THEMES.iter().position(|t| *t == self.config.theme.name).unwrap_or(0);
        let next = if forward { (cur + 1) % n } else { (cur + n - 1) % n };
        self.config.theme.name = BUILTIN_THEMES[next].to_string();
        self.after_config_change();
    }

    fn cycle_preset_dir(&mut self, forward: bool) {
        let names: Vec<String> = self.config.presets.iter().map(|p| p.name.clone()).collect();
        if names.is_empty() {
            return;
        }
        let n = names.len();
        let cur = names.iter().position(|x| *x == self.config.active_preset).unwrap_or(0);
        let next = if forward { (cur + 1) % n } else { (cur + n - 1) % n };
        self.config.active_preset = names[next].clone();
        self.after_config_change();
    }

    fn menu_key(&mut self, code: KeyCode) {
        let rows = self.menu_rows();
        match code {
            KeyCode::Esc | KeyCode::Char('m') => self.menu = None,
            KeyCode::Up => {
                if let Some(m) = self.menu.as_mut() {
                    if rows > 0 {
                        m.focus = (m.focus + rows - 1) % rows;
                    }
                }
            }
            KeyCode::Down => {
                if let Some(m) = self.menu.as_mut() {
                    if rows > 0 {
                        m.focus = (m.focus + 1) % rows;
                    }
                }
            }
            KeyCode::Left => self.menu_adjust(false),
            KeyCode::Right => self.menu_adjust(true),
            KeyCode::Enter | KeyCode::Char(' ') => self.menu_toggle(),
            _ => {}
        }
    }

    fn menu_adjust(&mut self, forward: bool) {
        let focus = match self.menu.as_ref() {
            Some(m) => m.focus,
            None => return,
        };
        match focus {
            0 => self.cycle_preset_dir(forward),
            1 => self.cycle_theme(forward),
            2 => {
                self.config.theme.borders = match self.config.theme.borders {
                    Borders::Rounded => Borders::Square,
                    Borders::Square => Borders::Rounded,
                };
                self.after_config_change();
            }
            3 => {
                self.config.theme.meters = match self.config.theme.meters {
                    Meters::Gradient => Meters::Solid,
                    Meters::Solid => Meters::Gradient,
                };
                self.after_config_change();
            }
            _ => {} // panel rows toggle with Enter/Space, not Left/Right
        }
    }

    fn menu_toggle(&mut self) {
        let focus = match self.menu.as_ref() {
            Some(m) => m.focus,
            None => return,
        };
        if focus < 4 {
            return;
        }
        let pidx = focus - 4;
        let Some(ai) = self.active_preset_index() else { return };
        let toggled = if let Some(spec) = self.config.presets[ai].panels.get_mut(pidx) {
            spec.hidden = !spec.hidden;
            true
        } else {
            false
        };
        if toggled {
            self.after_config_change();
        }
    }
```

- [ ] **Step 4: Wire `m` into `on_key`**

In `on_key`, add the menu dispatch right after the filter dispatch at the top:

```rust
        if self.menu.is_some() {
            self.menu_key(code);
            return;
        }
```

…and add the `m` binding among the Dashboard-mode arms:

```rust
            KeyCode::Char('m') if self.mode == Mode::Dashboard => {
                self.menu = Some(MenuState { focus: 0 })
            }
```

- [ ] **Step 5: Write the failing tests**

Add to `mod tests` in `app.rs`:

```rust
    #[test]
    fn m_opens_menu_at_first_row() {
        let mut app = app_with_default();
        app.on_key(KeyCode::Char('m'));
        assert_eq!(app.menu.as_ref().unwrap().focus, 0);
    }

    #[test]
    fn menu_cycles_theme_and_marks_dirty() {
        let mut app = app_with_default();
        app.config.theme.name = "tokyo-night".into();
        app.on_key(KeyCode::Char('m'));
        app.on_key(KeyCode::Down); // focus → theme (row 1)
        app.config_dirty = false;
        app.on_key(KeyCode::Right);
        assert_eq!(app.config.theme.name, "catppuccin-mocha");
        assert!(app.config_dirty);
        assert_eq!(app.theme.name, "catppuccin-mocha"); // resolved theme updated
    }

    #[test]
    fn menu_toggles_border_style() {
        let mut app = app_with_default();
        app.config.theme.borders = Borders::Rounded;
        app.on_key(KeyCode::Char('m'));
        app.on_key(KeyCode::Down);
        app.on_key(KeyCode::Down); // focus → borders (row 2)
        app.on_key(KeyCode::Right);
        assert_eq!(app.config.theme.borders, Borders::Square);
    }

    #[test]
    fn menu_hides_a_panel_and_rebuild_skips_it() {
        let mut app = app_with_default();
        assert_eq!(app.panels.len(), 10);
        app.on_key(KeyCode::Char('m'));
        // focus the first panel row (index 4)
        for _ in 0..4 {
            app.on_key(KeyCode::Down);
        }
        app.on_key(KeyCode::Enter); // toggle hidden on panel 0
        assert!(app.config.presets[0].panels[0].hidden);
        assert_eq!(app.panels.len(), 9, "hidden panel is skipped on rebuild");
        assert!(app.config_dirty);
    }

    #[test]
    fn esc_closes_menu() {
        let mut app = app_with_default();
        app.on_key(KeyCode::Char('m'));
        app.on_key(KeyCode::Esc);
        assert!(app.menu.is_none());
    }
```

- [ ] **Step 6: Run** — `cargo test -p gauge-client tui::app -- --nocapture`
Expected: PASS (menu tests + earlier app tests). `ui.rs`/`run.rs` still compile (new public fields default to `None`/`false`).

- [ ] **Step 7: Commit**

```bash
git add crates/gauge/src/tui/app.rs
git commit -m "feat(tui): live menu (preset/theme/border/meter/panel toggle) + dirty flag"
```

---

## Task 3: Persist on dirty in `run.rs`

**Files:**
- Modify: `crates/gauge/src/tui/run.rs`

- [ ] **Step 1: Persist in the loop**

In `event_loop`, just before the `if app.should_quit` check, add:

```rust
        if app.config_dirty {
            app.config_dirty = false;
            if let Err(e) = app.config.save() {
                app.config_error = Some(format!("could not save dashboard.toml: {e}"));
            }
        }
```

(`DashboardConfig::save` is the atomic write from Plan 1; it resolves `paths::dashboard_path()`.)

- [ ] **Step 2: Build**

Run: `cargo build -p gauge-client`
Expected: compiles. (No new test here — persistence writes real files; the atomic write is covered by Plan 1's `save_to`/`load_from` round-trip test. Behaviour is verified in the manual smoke at Task 6.)

- [ ] **Step 3: Commit**

```bash
git add crates/gauge/src/tui/run.rs
git commit -m "feat(tui): persist dashboard.toml after live menu edits"
```

---

## Task 4: Menu overlay in `ui.rs`

**Files:**
- Modify: `crates/gauge/src/tui/ui.rs`

- [ ] **Step 1: Add the renderer and call it**

Import `MenuState` is not needed (we read fields off `app`), but `app.menu` is used. In `render`, after the filter-overlay block, add:

```rust
    if app.menu.is_some() {
        render_menu_overlay(f, app, area);
    }
```

Add this function to `ui.rs`:

```rust
fn render_menu_overlay(f: &mut Frame, app: &App, area: Rect) {
    let Some(menu) = &app.menu else { return };
    let t = &app.theme;

    let mut items: Vec<String> = vec![
        format!("Preset:  {}", app.config.active_preset),
        format!("Theme:   {}", app.config.theme.name),
        format!("Borders: {:?}", app.config.theme.borders),
        format!("Meters:  {:?}", app.config.theme.meters),
    ];
    if let Some(preset) = app.config.active_preset() {
        for spec in &preset.panels {
            let mark = if spec.hidden { " " } else { "x" };
            let label = spec.title.clone().unwrap_or_else(|| spec.kind.clone());
            items.push(format!("[{mark}] {label}"));
        }
    }

    let popup = centered_rect(area, 48, (items.len() as u16 + 4).min(area.height));
    f.render_widget(Clear, popup);
    let block = panel_block("Menu", t).style(Style::default().bg(t.palette.surface));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let mut lines: Vec<Line> = Vec::new();
    list_lines(&mut lines, &items, menu.focus, t);
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " ↑↓ move · ←→ change · enter toggle panel · esc/m close",
        Style::default().fg(t.palette.muted),
    )));
    f.render_widget(Paragraph::new(lines), inner);
}
```

- [ ] **Step 2: Add `m` to the dashboard status hints**

In `render_status_bar`, update the Dashboard hint:

```rust
        Mode::Dashboard => "tab:explore   /:filter   c:clear   m:menu   p:preset   t:range   q:quit",
```

- [ ] **Step 3: Write the failing test**

Add to `mod tests` in `ui.rs`:

```rust
    #[test]
    fn menu_overlay_renders_when_open() {
        let mut a = app();
        a.on_key(crossterm::event::KeyCode::Char('m'));
        let out = draw(&a, 100, 30);
        assert!(out.contains("Menu"));
        assert!(out.contains("Preset:"));
        assert!(out.contains("Theme:"));
    }
```

- [ ] **Step 4: Run** — `cargo test -p gauge-client tui::ui -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/gauge/src/tui/ui.rs
git commit -m "feat(tui): live menu overlay + status hint"
```

---

## Task 5: Remove dead `data::{fetch,Snapshot}`

**Files:**
- Modify: `crates/gauge/src/tui/data.rs`

- [ ] **Step 1: Delete the unused items**

In `crates/gauge/src/tui/data.rs`, delete:
- the `pub struct Snapshot { .. }` definition, and
- the `pub async fn fetch(..) -> Result<Snapshot, ClientError>` function.

**Keep** `TimeWindow` (+ `doubled_last`), `base`, `nice_round`, `derive_edges`, `histogram_probe_request`, `histogram_bucket_request`, `fetch_histogram`, `numeric_attr_field`, the `QuerySource`/`collect_requests`/`fetch_all` additions, and all existing histogram tests.

- [ ] **Step 2: Fix resulting unused imports**

Run: `cargo build -p gauge-client`
Then: `cargo clippy -p gauge-client --all-targets -- -D warnings`
Removing `fetch`/`Snapshot` likely makes some `gauge_query` imports unused in `data.rs` (e.g. `AppMeta`, `Order`, `Dir`, `Granularity` if only `fetch` used them). Remove each import clippy flags as unused until clean. Confirm `base` still compiles (it is used by the histogram request builders).

- [ ] **Step 3: Run the full suite**

Run: `cargo test -p gauge-client`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/gauge/src/tui/data.rs
git commit -m "refactor(tui): drop dead Snapshot/fetch (superseded by panel data layer)"
```

---

## Task 6: README + final gate + manual smoke

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Rewrite the `### TUI` subsection**

In `README.md`, replace the existing `### TUI` subsection (from the `### TUI` heading up to, but not including, the `### MCP server` heading) with:

```markdown
### TUI

`gauge tui` opens a configurable, themed dashboard in the `btm`/`sampler` idiom.

**Two modes** (`tab` toggles):

- **Dashboard** — a grid of panels defined in `dashboard.toml`; presets switch whole
  layouts.
- **Explore** — an interactive query builder over the DSL.

**Panels:** `timeseries`, `stat` (big number + Δ vs previous period + sparkline),
`top_n`, `breakdown`, `numeric_stats`, `histogram`, and `apps_table`. The default
layout shows activity over time, stat tiles, top events, a latency distribution, and
OS/arch/version breakdowns.

**Customize** via `~/.config/gauge/dashboard.toml` (theme, presets, panels, per-panel
filter pins) and live in-app: `m` opens a menu to switch preset/theme/border/meter
style and show/hide panels — changes persist back to the file.

**Filter** the whole dashboard: `/` adds a filter (field → op → value, values
suggested from `/v1/meta`), `c` clears. Filters cover `app`, `event_name`, `os`,
`arch`, `app_version`, and any `attr.<key>` (`=`/`≠`/`in`/`exists`/`>`/`<`);
`install_id`/`session_id` stay non-filterable for anonymity.

**Themes:** Tokyo Night (default), Catppuccin Mocha, Gruvbox Dark, Nord, plus `ansi`
(inherits your terminal's 16 colours) and custom palettes in config.

Rendering is pure and decoupled from polling (default 30s; `r` forces a refresh), so a
slow network never blocks the UI — failures degrade to a stale-data banner. Keys:
`q` quit · `tab` mode · `t` time range · `p` preset · `/` filter · `c` clear · `m` menu
· arrows navigate.
```

- [ ] **Step 2: Final gate**

Run: `cargo build -p gauge-client` → compiles.
Run: `cargo clippy -p gauge-client --all-targets -- -D warnings` → clean.
Run: `cargo test -p gauge-client` → all pass.

- [ ] **Step 3: Manual smoke (if a server is available)**

`cargo run -p gauge-client --bin gauge -- tui`, then:
- `m` opens the menu; `←/→` on Theme cycles palettes live; on Borders toggles rounded/square; arrow to a panel row and `enter` hides/shows it.
- Quit and reopen `gauge tui` — the changes persisted (check `~/.config/gauge/dashboard.toml` exists and reflects the edits).
- `/` adds a filter, `c` clears.

If no server is available, record the manual step as **deferred**; do not claim end-to-end success without it.

- [ ] **Step 4: Commit**

```bash
git add README.md
git commit -m "docs(readme): document the redesigned configurable TUI dashboard"
```

---

## Done criteria for Plan 5

- `m` opens a menu that switches preset, cycles theme, toggles border/meter style, and shows/hides panels; every edit persists to `dashboard.toml` (atomic write) and re-renders.
- `data::{fetch,Snapshot}` removed; the old polling path is fully replaced by the panel data layer.
- README documents the new dashboard.
- `cargo test` and `cargo clippy --all-targets -- -D warnings` clean.

## Series complete

With Plans 1–5 implemented, `gauge tui` is the configurable, themed, filterable dashboard from the design spec (`docs/superpowers/specs/2026-06-18-tui-dashboard-redesign-design.md`), entirely client-side, privacy preserved.

**Explicitly deferred (future work, not in this series):** in-menu panel *add/remove/reorder* (config-file-only for now), an absolute date-range picker, auto-fit histogram edges in the dashboard `histogram` panel (Explore already auto-fits), and mouse interaction.
