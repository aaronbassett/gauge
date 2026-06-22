//! The `gauge status` sparkline-wave art. Original art (no third-party
//! source). Each returned line is exactly `ART_WIDTH` VISIBLE columns wide so
//! the column zipper can place the panel at a stable offset without measuring
//! ANSI escapes. Non-space glyphs cycle through the supplied palette accents.

use ratatui::style::Color;

use crate::term;

/// Visible width of the frame interior (between the `│` borders).
pub const INTERIOR: usize = 23;
/// Visible width of every art line: `│` + interior + `│`.
pub const ART_WIDTH: usize = INTERIOR + 2;

// Wave rows. Exact source spacing is forgiving — `fit` pads/truncates each to
// INTERIOR before framing. Tune visually; widths are normalised in code.
const ROWS: [&str; 3] = [
    "        ╱╲    ╱╲        ",
    "  ╱╲╱╲╱   ╲╱╲╱   ╲╱╲    ",
    " ╱╲              ╲╱╲    ",
];

/// Pad (with spaces) or truncate `row` to exactly `INTERIOR` visible chars.
/// Char-based so multi-byte box-drawing glyphs each count as one column.
pub fn fit(row: &str) -> String {
    let mut s: String = row.chars().take(INTERIOR).collect();
    let n = s.chars().count();
    if n < INTERIOR {
        s.push_str(&" ".repeat(INTERIOR - n));
    }
    s
}

/// ANSI foreground escape for a palette colour, or `""` for colours we don't
/// map (e.g. `Reset`/`DarkGray`) — the glyph then renders in the default fg.
pub fn fg(c: Color) -> String {
    match c {
        Color::Rgb(r, g, b) => format!("\x1b[38;2;{r};{g};{b}m"),
        Color::Red => "\x1b[31m".into(),
        Color::Green => "\x1b[32m".into(),
        Color::Yellow => "\x1b[33m".into(),
        Color::Blue => "\x1b[34m".into(),
        Color::Magenta => "\x1b[35m".into(),
        Color::Cyan => "\x1b[36m".into(),
        Color::LightBlue => "\x1b[94m".into(),
        _ => String::new(),
    }
}

/// Colour each non-space glyph by its position, cycling through `accents`.
/// Spaces pass through. Plain (no ANSI) when colour is disabled or there are
/// no accents — which keeps the visible width equal to the input width.
fn paint(row: &str, accents: &[Color]) -> String {
    if !term::color_enabled() || accents.is_empty() {
        return row.to_string();
    }
    let mut out = String::new();
    let mut i = 0usize;
    for ch in row.chars() {
        if ch == ' ' {
            out.push(' ');
            continue;
        }
        out.push_str(&fg(accents[i % accents.len()]));
        out.push(ch);
        out.push_str("\x1b[0m");
        i += 1;
    }
    out
}

/// The framed sparkline: top border, 3 wave rows, bottom border — five lines,
/// each `ART_WIDTH` visible columns wide, top-aligned.
pub fn sparkline(accents: &[Color]) -> Vec<String> {
    let bar = "─".repeat(INTERIOR);
    let mut lines = vec![format!("┌{bar}┐")];
    for row in ROWS {
        lines.push(format!("│{}│", paint(&fit(row), accents)));
    }
    lines.push(format!("└{bar}┘"));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    #[test]
    fn lines_are_exactly_art_width_visible() {
        // No accents → no ANSI, so char count == visible width.
        let lines = sparkline(&[]);
        assert_eq!(lines.len(), 5, "top + 3 wave rows + bottom");
        for l in &lines {
            assert_eq!(l.chars().count(), ART_WIDTH, "line {l:?} not ART_WIDTH wide");
        }
    }

    #[test]
    fn fit_pads_and_truncates_to_interior() {
        assert_eq!(fit("ab").chars().count(), INTERIOR);
        assert_eq!(fit(&"x".repeat(100)).chars().count(), INTERIOR);
    }

    #[test]
    fn fg_maps_rgb_to_truecolor_escape() {
        assert_eq!(fg(Color::Rgb(1, 2, 3)), "\x1b[38;2;1;2;3m");
        assert_eq!(fg(Color::Reset), "");
    }
}
