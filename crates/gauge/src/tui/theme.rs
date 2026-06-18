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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_parses_and_rejects() {
        assert_eq!(parse_hex_color("#1a1b26"), Some(Color::Rgb(0x1a, 0x1b, 0x26)));
        assert_eq!(parse_hex_color("#FFFFFF"), Some(Color::Rgb(255, 255, 255)));
        assert_eq!(parse_hex_color("1a1b26"), None);
        assert_eq!(parse_hex_color("#abc"), None);
        assert_eq!(parse_hex_color("#gggggg"), None);
    }

    #[test]
    fn border_style_maps() {
        assert_eq!(BorderStyle::Rounded.border_type(), BorderType::Rounded);
        assert_eq!(BorderStyle::Square.border_type(), BorderType::Plain);
    }
}
