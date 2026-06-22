//! Plain-text terminal helpers for non-TUI command output: TTY detection,
//! colour gating (TTY && no `NO_COLOR`), terminal width, and semantic ANSI
//! wrappers. Colour lives in the ratatui TUI for the dashboard; this is the
//! CLI-side equivalent for `gauge status`.

use std::io::IsTerminal as _;

const RESET: &str = "\x1b[0m";

/// True when stdout is an interactive terminal.
pub fn stdout_is_tty() -> bool {
    std::io::stdout().is_terminal()
}

/// Colour is enabled only on a TTY and when `NO_COLOR` is unset
/// (https://no-color.org/).
pub fn color_enabled() -> bool {
    stdout_is_tty() && std::env::var_os("NO_COLOR").is_none()
}

/// Best-effort terminal width: `$COLUMNS` if a positive integer, else 80.
pub fn term_width() -> usize {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok())
        .filter(|w| *w > 0)
        .unwrap_or(80)
}

fn wrap(code: &str, s: &str) -> String {
    if color_enabled() {
        format!("\x1b[{code}m{s}{RESET}")
    } else {
        s.to_string()
    }
}

pub fn bold(s: &str) -> String {
    wrap("1", s)
}
pub fn dim(s: &str) -> String {
    wrap("2", s)
}
/// Field-label colour (cyan) for the status panel keys.
pub fn label(s: &str) -> String {
    wrap("36", s)
}
pub fn green(s: &str) -> String {
    wrap("32", s)
}
pub fn yellow(s: &str) -> String {
    wrap("33", s)
}
pub fn red(s: &str) -> String {
    wrap("31", s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_disabled_in_non_tty_test_process() {
        // The test process has no TTY on stdout, so colour is off and the
        // wrappers must return their input unchanged (no ANSI escapes).
        assert_eq!(bold("hi"), "hi");
        assert_eq!(green("ok"), "ok");
        assert!(!color_enabled());
    }

    #[test]
    fn term_width_reads_columns_env_else_80() {
        unsafe { std::env::set_var("COLUMNS", "123") };
        assert_eq!(term_width(), 123);
        unsafe { std::env::remove_var("COLUMNS") };
        assert_eq!(term_width(), 80);
    }
}
