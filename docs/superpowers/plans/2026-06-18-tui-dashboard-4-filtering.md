# TUI Dashboard Redesign — Plan 4: Filtering

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the global filter bar live. Add a `/`-triggered overlay that walks **field → operator → value** (values for `app`/`event_name` discovered from `/v1/meta`, others free-text/numeric), commits a `gauge_query::Filter` into `app.filters`, and refreshes; `c` clears all filters. Chips already render in the top bar (Plan 3), and `app.filters` already flows through `PanelCtx` into every panel request (Plan 2/3) — this plan fills `app.filters`.

**Architecture:** A small modal state machine in `app.rs` (`FilterDraft` with `FilterStep::{Field,Op,Value}`). While the overlay is open, all keys route to the filter handler; otherwise `/` opens it and `c` clears. `ui.rs` draws a centered popup. Privacy is preserved structurally: the field candidate list is built only from addressable fields + meta attribute keys, so `install_id`/`session_id` can never be entered (and `Field::parse` would reject them anyway).

**Tech Stack:** Rust 2024, `ratatui 0.29` (`Clear` for the popup), `gauge-query` (`Filter`/`FilterOp`/`FilterValue`/`Field`).

**Plan series:** Plan 4 of 5. Depends on Plan 3. After this, filtering works end to end. Plan 5 adds the live menu (`m`) + persistence.

---

## File Structure

| File | Responsibility |
|---|---|
| `crates/gauge/src/tui/app.rs` | **Modify.** `FilterStep`, `FilterDraft`; `App.filter_input`; `open_filter`/`filter_key`/`filter_advance`/`commit_filter`; `filter_fields`/`ops_for`/`values_for`; `on_key` dispatch + `/` and `c` bindings. |
| `crates/gauge/src/tui/ui.rs` | **Modify.** `render_filter_overlay` (centered popup) + call from `render`; add `/`/`c` to the dashboard status hints. |

---

## Task 1: Filter modal state machine in `app.rs`

**Files:**
- Modify: `crates/gauge/src/tui/app.rs`

- [ ] **Step 1: Add the draft types and the `App` field**

Add near the top of `app.rs` (after `Mode`):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterStep {
    Field,
    Op,
    Value,
}

/// In-progress filter being entered through the overlay.
#[derive(Debug, Clone)]
pub struct FilterDraft {
    pub step: FilterStep,
    pub fields: Vec<String>,
    pub field_idx: usize,
    pub field: Option<String>,
    pub ops: Vec<gauge_query::FilterOp>,
    pub op_idx: usize,
    pub op: Option<gauge_query::FilterOp>,
    pub values: Vec<String>,
    pub value_idx: usize,
    pub buffer: String,
}
```

Add a field to `App`:

```rust
    pub filter_input: Option<FilterDraft>,
```

…and initialise it in `App::new` (alongside the other fields): `filter_input: None,`.

Extend the imports at the top of `app.rs` to bring in the filter types:

```rust
use gauge_query::{AppMeta, Filter, FilterOp, FilterValue, QueryRequest};
```

- [ ] **Step 2: Add the overlay logic**

Add these methods inside `impl App`:

```rust
    /// Addressable filter fields + meta attribute keys. Never includes the
    /// non-addressable `install_id`/`session_id` (privacy).
    pub fn filter_fields(&self) -> Vec<String> {
        let mut v: Vec<String> = ["app", "event_name", "os", "arch", "app_version"]
            .into_iter()
            .map(String::from)
            .collect();
        let mut attrs: Vec<String> = self
            .meta
            .iter()
            .flat_map(|m| m.attribute_keys.iter().cloned())
            .collect();
        attrs.sort_unstable();
        attrs.dedup();
        v.extend(attrs.into_iter().map(|k| format!("attr.{k}")));
        v
    }

    fn ops_for(&self, field: &str) -> Vec<FilterOp> {
        let numeric = field
            .strip_prefix("attr.")
            .map(|k| {
                self.meta
                    .iter()
                    .any(|m| m.numeric_attribute_keys.iter().any(|n| n == k))
            })
            .unwrap_or(false);
        if numeric {
            vec![
                FilterOp::Eq,
                FilterOp::Neq,
                FilterOp::Gt,
                FilterOp::Gte,
                FilterOp::Lt,
                FilterOp::Lte,
                FilterOp::Exists,
            ]
        } else {
            vec![FilterOp::Eq, FilterOp::Neq, FilterOp::In, FilterOp::Exists]
        }
    }

    fn values_for(&self, field: &str) -> Vec<String> {
        let mut v: Vec<String> = match field {
            "app" => self.meta.iter().map(|m| m.app.clone()).collect(),
            "event_name" => self.meta.iter().flat_map(|m| m.event_names.iter().cloned()).collect(),
            _ => vec![],
        };
        v.sort_unstable();
        v.dedup();
        v
    }

    fn open_filter(&mut self) {
        let fields = self.filter_fields();
        self.filter_input = Some(FilterDraft {
            step: FilterStep::Field,
            fields,
            field_idx: 0,
            field: None,
            ops: vec![],
            op_idx: 0,
            op: None,
            values: vec![],
            value_idx: 0,
            buffer: String::new(),
        });
    }

    fn filter_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => self.filter_input = None,
            KeyCode::Enter => self.filter_advance(),
            KeyCode::Up | KeyCode::Down => {
                let down = code == KeyCode::Down;
                if let Some(d) = self.filter_input.as_mut() {
                    let len = match d.step {
                        FilterStep::Field => d.fields.len(),
                        FilterStep::Op => d.ops.len(),
                        FilterStep::Value => d.values.len(),
                    };
                    if len > 0 {
                        let idx = match d.step {
                            FilterStep::Field => &mut d.field_idx,
                            FilterStep::Op => &mut d.op_idx,
                            FilterStep::Value => &mut d.value_idx,
                        };
                        *idx = if down { (*idx + 1) % len } else { (*idx + len - 1) % len };
                    }
                }
            }
            KeyCode::Backspace => {
                if let Some(d) = self.filter_input.as_mut() {
                    if d.step == FilterStep::Value {
                        d.buffer.pop();
                    }
                }
            }
            KeyCode::Char(c) => {
                if let Some(d) = self.filter_input.as_mut() {
                    if d.step == FilterStep::Value {
                        d.buffer.push(c);
                    }
                }
            }
            _ => {}
        }
    }

    fn filter_advance(&mut self) {
        let step = match self.filter_input.as_ref() {
            Some(d) => d.step,
            None => return,
        };
        match step {
            FilterStep::Field => {
                let field = self.filter_input.as_ref().and_then(|d| d.fields.get(d.field_idx).cloned());
                if let Some(f) = field {
                    let ops = self.ops_for(&f);
                    if let Some(d) = self.filter_input.as_mut() {
                        d.field = Some(f);
                        d.ops = ops;
                        d.op_idx = 0;
                        d.step = FilterStep::Op;
                    }
                }
            }
            FilterStep::Op => {
                let op = self.filter_input.as_ref().and_then(|d| d.ops.get(d.op_idx).copied());
                let field = self.filter_input.as_ref().and_then(|d| d.field.clone());
                if let Some(op) = op {
                    if op == FilterOp::Exists {
                        if let Some(d) = self.filter_input.as_mut() {
                            d.op = Some(op);
                        }
                        self.commit_filter(None);
                    } else if let Some(f) = field {
                        let values = self.values_for(&f);
                        if let Some(d) = self.filter_input.as_mut() {
                            d.op = Some(op);
                            d.values = values;
                            d.value_idx = 0;
                            d.buffer.clear();
                            d.step = FilterStep::Value;
                        }
                    }
                }
            }
            FilterStep::Value => {
                let chosen = self.filter_input.as_ref().and_then(|d| {
                    if !d.buffer.is_empty() {
                        Some(d.buffer.clone())
                    } else {
                        d.values.get(d.value_idx).cloned()
                    }
                });
                if chosen.is_some() {
                    self.commit_filter(chosen);
                }
            }
        }
    }

    /// Build and push the filter, then close the overlay and refresh. Aborts (closing
    /// the overlay) on an unparseable numeric value or unknown field.
    fn commit_filter(&mut self, value: Option<String>) {
        let (field_s, op) = match self.filter_input.as_ref() {
            Some(d) => (d.field.clone(), d.op.unwrap_or(FilterOp::Eq)),
            None => return,
        };
        self.filter_input = None;
        let Some(field_s) = field_s else { return };
        let Ok(field) = gauge_query::Field::parse(&field_s) else { return };
        let value = match op {
            FilterOp::Exists => None,
            FilterOp::Gt | FilterOp::Gte | FilterOp::Lt | FilterOp::Lte => {
                match value.as_deref().and_then(|s| s.parse::<f64>().ok()) {
                    Some(n) => Some(FilterValue::Num(n)),
                    None => return, // invalid number → abort
                }
            }
            FilterOp::In => Some(FilterValue::Many(
                value
                    .unwrap_or_default()
                    .split(',')
                    .map(|x| x.trim().to_string())
                    .filter(|x| !x.is_empty())
                    .collect(),
            )),
            FilterOp::Eq | FilterOp::Neq => value.map(FilterValue::One),
        };
        self.filters.push(Filter { field, op, value });
        self.refresh_requested = true;
    }
```

- [ ] **Step 3: Wire `on_key` dispatch + `/` and `c`**

Replace the body of `on_key` so it routes to the overlay when open, and binds `/`/`c` in Dashboard mode. The full method becomes:

```rust
    pub fn on_key(&mut self, code: KeyCode) {
        if self.filter_input.is_some() {
            self.filter_key(code);
            return;
        }
        match code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Tab => {
                self.mode = match self.mode {
                    Mode::Dashboard => Mode::Explore,
                    Mode::Explore => Mode::Dashboard,
                }
            }
            KeyCode::Char('t') => {
                self.window = self.window.next();
                self.refresh_requested = true;
                self.explore.histogram = None;
            }
            KeyCode::Char('r') => self.refresh_requested = true,
            KeyCode::Char('p') if self.mode == Mode::Dashboard => self.cycle_preset(),
            KeyCode::Char('/') if self.mode == Mode::Dashboard => self.open_filter(),
            KeyCode::Char('c') if self.mode == Mode::Dashboard => {
                if !self.filters.is_empty() {
                    self.filters.clear();
                    self.refresh_requested = true;
                }
            }
            KeyCode::Up if self.mode == Mode::Explore => {
                self.explore.measure_idx = (self.explore.measure_idx + 1) % EXPLORE_MEASURES.len()
            }
            KeyCode::Down if self.mode == Mode::Explore => {
                self.explore.dimension_idx =
                    (self.explore.dimension_idx + 1) % EXPLORE_DIMENSIONS.len()
            }
            KeyCode::Enter if self.mode == Mode::Explore => self.explore.run_requested = true,
            KeyCode::Char('h')
                if self.mode == Mode::Explore && self.explore.numeric_attr.is_some() =>
            {
                self.explore.histogram_requested = true
            }
            KeyCode::Char('n') if self.mode == Mode::Explore => self.cycle_numeric_attr(),
            _ => {}
        }
    }
```

- [ ] **Step 4: Write the failing tests**

Add to `mod tests` in `app.rs`:

```rust
    fn app_with_meta() -> App {
        let mut app = app_with_default();
        app.meta = vec![AppMeta {
            app: "tome".into(),
            event_names: vec!["build".into(), "test".into()],
            attribute_keys: vec!["latency_ms".into(), "surface".into()],
            numeric_attribute_keys: vec!["latency_ms".into()],
            first_event: None,
            last_event: None,
            total_events: 0,
        }];
        app
    }

    #[test]
    fn filter_fields_exclude_identifying_fields() {
        let app = app_with_meta();
        let fields = app.filter_fields();
        assert!(fields.contains(&"app".to_string()));
        assert!(fields.contains(&"attr.latency_ms".to_string()));
        assert!(!fields.iter().any(|f| f == "install_id" || f == "session_id"));
    }

    #[test]
    fn slash_walks_field_op_value_and_commits_a_filter() {
        let mut app = app_with_meta();
        app.on_key(KeyCode::Char('/'));
        assert!(app.filter_input.is_some());
        // field: "app" is first
        app.on_key(KeyCode::Enter); // choose app → Op step
        assert_eq!(app.filter_input.as_ref().unwrap().step, FilterStep::Op);
        // op: Eq is first
        app.on_key(KeyCode::Enter); // choose Eq → Value step
        assert_eq!(app.filter_input.as_ref().unwrap().step, FilterStep::Value);
        // value: "tome" is the only suggestion
        app.on_key(KeyCode::Enter);
        assert!(app.filter_input.is_none());
        assert_eq!(app.filters.len(), 1);
        assert_eq!(app.filters[0].field, gauge_query::Field::App);
        assert_eq!(app.filters[0].op, FilterOp::Eq);
        assert!(matches!(&app.filters[0].value, Some(FilterValue::One(s)) if s == "tome"));
        assert!(app.refresh_requested);
    }

    #[test]
    fn exists_op_commits_without_a_value() {
        let mut app = app_with_meta();
        app.open_filter();
        app.on_key(KeyCode::Enter); // app → Op
        // move op selection to Exists (last in [Eq,Neq,In,Exists] = index 3)
        for _ in 0..3 {
            app.on_key(KeyCode::Down);
        }
        app.on_key(KeyCode::Enter);
        assert!(app.filter_input.is_none());
        assert_eq!(app.filters.len(), 1);
        assert_eq!(app.filters[0].op, FilterOp::Exists);
        assert!(app.filters[0].value.is_none());
    }

    #[test]
    fn numeric_attr_gt_commits_a_number_via_typed_buffer() {
        let mut app = app_with_meta();
        app.open_filter();
        // navigate to attr.latency_ms (the numeric attr; not necessarily the last field)
        let li = app
            .filter_input
            .as_ref()
            .unwrap()
            .fields
            .iter()
            .position(|f| f == "attr.latency_ms")
            .unwrap();
        for _ in 0..li {
            app.on_key(KeyCode::Down);
        }
        app.on_key(KeyCode::Enter); // → Op (numeric ops)
        // ops = [Eq,Neq,Gt,Gte,Lt,Lte,Exists]; move to Gt (index 2)
        app.on_key(KeyCode::Down);
        app.on_key(KeyCode::Down);
        app.on_key(KeyCode::Enter); // → Value
        for c in "100".chars() {
            app.on_key(KeyCode::Char(c));
        }
        app.on_key(KeyCode::Enter);
        assert_eq!(app.filters.len(), 1);
        assert_eq!(app.filters[0].op, FilterOp::Gt);
        assert!(matches!(app.filters[0].value, Some(FilterValue::Num(n)) if (n - 100.0).abs() < 1e-9));
    }

    #[test]
    fn c_clears_all_filters() {
        let mut app = app_with_meta();
        app.filters.push(Filter { field: gauge_query::Field::Os, op: FilterOp::Exists, value: None });
        app.refresh_requested = false;
        app.on_key(KeyCode::Char('c'));
        assert!(app.filters.is_empty());
        assert!(app.refresh_requested);
    }

    #[test]
    fn esc_cancels_the_overlay_without_adding_a_filter() {
        let mut app = app_with_meta();
        app.open_filter();
        app.on_key(KeyCode::Esc);
        assert!(app.filter_input.is_none());
        assert!(app.filters.is_empty());
    }
```

- [ ] **Step 5: Run** — `cargo test -p gauge-client tui::app -- --nocapture`
Expected: PASS (the filter tests + the Plan 3 app tests). `ui.rs` still compiles because `filter_input` is a new public field with `None` default — the overlay render is added in Task 2.

- [ ] **Step 6: Commit**

```bash
git add crates/gauge/src/tui/app.rs
git commit -m "feat(tui): global filter bar state machine (field/op/value, clear)"
```

---

## Task 2: Filter overlay rendering in `ui.rs`

**Files:**
- Modify: `crates/gauge/src/tui/ui.rs`

- [ ] **Step 1: Add the overlay renderer and call it**

Add `Clear` to the `ratatui::widgets` import in `ui.rs`:

```rust
use ratatui::widgets::{Bar, BarChart, BarGroup, Block, Clear, Paragraph};
```

Import the filter types from `app`:

```rust
use crate::tui::app::{App, EXPLORE_DIMENSIONS, EXPLORE_MEASURES, FilterStep, Mode, NUMERIC_MEASURE_BASE};
```

In `render`, after `render_status_bar(...)`, add:

```rust
    if app.filter_input.is_some() {
        render_filter_overlay(f, app, area);
    }
```

Add these functions to `ui.rs`:

```rust
fn centered_rect(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    }
}

fn op_label(op: FilterOp) -> &'static str {
    match op {
        FilterOp::Eq => "= (eq)",
        FilterOp::Neq => "≠ (neq)",
        FilterOp::In => "in",
        FilterOp::Exists => "? (exists)",
        FilterOp::Gt => "> (gt)",
        FilterOp::Gte => "≥ (gte)",
        FilterOp::Lt => "< (lt)",
        FilterOp::Lte => "≤ (lte)",
    }
}

fn list_lines<'a>(out: &mut Vec<Line<'a>>, items: &[String], selected: usize, theme: &crate::tui::theme::Theme) {
    for (i, item) in items.iter().enumerate() {
        let style = if i == selected {
            Style::default().fg(theme.palette.bg).bg(theme.palette.accents[0])
        } else {
            Style::default().fg(theme.palette.text)
        };
        out.push(Line::from(Span::styled(format!(" {} {item}", if i == selected { "▸" } else { " " }), style)));
    }
}

fn render_filter_overlay(f: &mut Frame, app: &App, area: Rect) {
    let Some(d) = &app.filter_input else { return };
    let t = &app.theme;
    let popup = centered_rect(area, 50, 16);
    f.render_widget(Clear, popup);

    let title = match d.step {
        FilterStep::Field => "Add filter — field",
        FilterStep::Op => "Add filter — operator",
        FilterStep::Value => "Add filter — value",
    };
    let block = panel_block(title, t).style(Style::default().bg(t.palette.surface));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let mut lines: Vec<Line> = Vec::new();
    match d.step {
        FilterStep::Field => list_lines(&mut lines, &d.fields, d.field_idx, t),
        FilterStep::Op => {
            let labels: Vec<String> = d.ops.iter().map(|o| op_label(*o).to_string()).collect();
            list_lines(&mut lines, &labels, d.op_idx, t);
        }
        FilterStep::Value => {
            lines.push(Line::from(Span::styled(
                format!(" value: {}", d.buffer),
                Style::default().fg(t.palette.text).add_modifier(Modifier::BOLD),
            )));
            if !d.values.is_empty() {
                lines.push(Line::from(Span::styled(
                    " suggestions (↑↓), or type:",
                    Style::default().fg(t.palette.muted),
                )));
                list_lines(&mut lines, &d.values, d.value_idx, t);
            } else {
                lines.push(Line::from(Span::styled(
                    " type a value",
                    Style::default().fg(t.palette.muted),
                )));
            }
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " ↑↓ select · enter confirm · esc cancel",
        Style::default().fg(t.palette.muted),
    )));
    f.render_widget(Paragraph::new(lines), inner);
}
```

- [ ] **Step 2: Update the dashboard status hints**

In `render_status_bar`, change the Dashboard hint string to include filtering:

```rust
        Mode::Dashboard => "tab:explore   /:filter   c:clear   p:preset   t:range   q:quit",
```

- [ ] **Step 3: Write the failing test**

Add to `mod tests` in `ui.rs`:

```rust
    #[test]
    fn filter_overlay_renders_when_open() {
        let mut a = app();
        a.meta = vec![gauge_query::AppMeta {
            app: "tome".into(),
            event_names: vec![],
            attribute_keys: vec![],
            numeric_attribute_keys: vec![],
            first_event: None,
            last_event: None,
            total_events: 0,
        }];
        a.on_key(crossterm::event::KeyCode::Char('/'));
        let out = draw(&a, 100, 30);
        assert!(out.contains("Add filter"));
        assert!(out.contains("app")); // first field candidate
    }
```

- [ ] **Step 4: Run** — `cargo test -p gauge-client tui::ui -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/gauge/src/tui/ui.rs
git commit -m "feat(tui): filter overlay popup + status hints"
```

---

## Task 3: Filtering gate

**Files:** none (verification only)

- [ ] **Step 1: Build** — `cargo build -p gauge-client` → compiles.
- [ ] **Step 2: Clippy** — `cargo clippy -p gauge-client --all-targets -- -D warnings` → clean (watch for `needless_range_loop` in `list_lines`; the index drives both selection styling and the marker, so it's justified — if clippy flags it, add `#[allow(clippy::needless_range_loop)]` with a one-line comment, or restructure with `.enumerate()` which the code already uses).
- [ ] **Step 3: Tests** — `cargo test -p gauge-client` → all pass.
- [ ] **Step 4: Manual smoke (if a server is available)** — `cargo run -p gauge-client --bin gauge -- tui`, then `/` → pick `app` → `=` → a value; confirm a chip appears in the top bar and panels reload scoped to it; `c` clears. If no server, record as deferred.
- [ ] **Step 5: Commit any fixes** — `git commit -am "chore(tui): filtering passes build + clippy -D warnings"`.

---

## Done criteria for Plan 4

- `/` opens the overlay; field→op→value commits a `Filter` into `app.filters`; `exists` commits with no value; numeric attrs accept comparison ops with a typed number; `c` clears; `esc` cancels.
- Filters render as chips (top bar) and scope every panel (already wired through `ctx`).
- `install_id`/`session_id` are never offered and cannot be entered (privacy guard test passes).
- `cargo test` and `cargo clippy --all-targets -- -D warnings` clean.

**Next:** Plan 5 adds the live menu (`m`) for switching presets / toggling panels / changing theme + border/meter style, persists edits back to `dashboard.toml` (atomic write from Plan 1), updates the README, and removes the now-dead `data::{fetch,Snapshot}`.
