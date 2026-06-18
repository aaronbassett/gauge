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

    #[test]
    fn tokyo_night_is_the_default_and_correct() {
        let p = builtin_palette("tokyo-night").expect("tokyo-night exists");
        assert_eq!(p.bg, Color::Rgb(0x1a, 0x1b, 0x26));
        assert_eq!(p.down, Color::Rgb(0xf7, 0x76, 0x8e));
        assert!(p.accents.len() >= 2);
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
        assert_eq!(builtin_palette("ansi").unwrap().bg, Color::Reset);
    }

    #[test]
    fn unknown_palette_is_none() {
        assert!(builtin_palette("not-a-theme").is_none());
    }
}
