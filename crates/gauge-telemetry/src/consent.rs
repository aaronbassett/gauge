//! Opt-out consent resolution. Pure: callers (the client builder) read the
//! process environment and pass values in. Disabled wins.

use std::time::{Duration, SystemTime};

/// The global kill-switch env var. `=1` forces telemetry off for ANY app.
pub const GLOBAL_DISABLE_VAR: &str = "GAUGE_TELEMETRY_DISABLE";

/// Inputs to the resolver, all already read from the environment/config.
#[derive(Debug, Clone, Default)]
pub struct ConsentInputs<'a> {
    /// Value of `GAUGE_TELEMETRY_DISABLE`, if set.
    pub global_disable: Option<&'a str>,
    /// Value of the app's own opt-out env var (e.g. `TOME_TELEMETRY`), if set.
    pub app_var: Option<&'a str>,
    /// The app config flag (true = user has not disabled in config).
    pub config_enabled: bool,
    /// The runtime toggle (true = not toggled off this run).
    pub runtime_enabled: bool,
    /// Whether a CI environment was detected.
    pub is_ci: bool,
}

/// Resolve whether telemetry is enabled. Disabled wins; the global var can only
/// ever disable (`=1`), never force-enable.
pub fn resolve(i: &ConsentInputs<'_>) -> bool {
    if i.global_disable == Some("1") {
        return false;
    }
    if !i.runtime_enabled || !i.config_enabled {
        return false;
    }
    if is_falsey(i.app_var) {
        return false;
    }
    if i.is_ci {
        return false;
    }
    true
}

/// `Some("0"|"false"|"off"|"no")` (case-insensitive) is a disable signal.
/// Unset or any other value is NOT a disable signal (and never force-enables).
fn is_falsey(v: Option<&str>) -> bool {
    matches!(
        v.map(|s| s.trim().to_ascii_lowercase()).as_deref(),
        Some("0" | "false" | "off" | "no")
    )
}

/// Detect CI from the conventional `CI` env var value.
pub fn is_ci(ci_var: Option<&str>) -> bool {
    !is_falsey_or_unset(ci_var)
}

fn is_falsey_or_unset(v: Option<&str>) -> bool {
    matches!(
        v.map(|s| s.trim().to_ascii_lowercase()).as_deref(),
        None | Some("" | "0" | "false" | "off" | "no")
    )
}

/// True while still inside the first-run grace window (flush should wait).
pub fn within_grace(mint: Option<SystemTime>, grace: Duration, now: SystemTime) -> bool {
    let Some(mint) = mint else {
        return false; // unknown mint time → don't hold delivery
    };
    match now.duration_since(mint) {
        Ok(elapsed) => elapsed < grace,
        Err(_) => true, // clock skew (mint in the future) → still within grace
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> ConsentInputs<'static> {
        ConsentInputs {
            config_enabled: true,
            runtime_enabled: true,
            ..Default::default()
        }
    }

    #[test]
    fn enabled_by_default() {
        assert!(resolve(&base()));
    }

    #[test]
    fn global_kill_switch_overrides_app_opt_in() {
        let i = ConsentInputs {
            global_disable: Some("1"),
            app_var: Some("1"),
            ..base()
        };
        assert!(!resolve(&i));
    }

    #[test]
    fn global_zero_does_not_force_enable() {
        let i = ConsentInputs {
            global_disable: Some("0"),
            app_var: Some("0"),
            ..base()
        };
        assert!(!resolve(&i));
    }

    #[test]
    fn app_falsey_disables() {
        for v in ["0", "false", "OFF", "No"] {
            let i = ConsentInputs {
                app_var: Some(v),
                ..base()
            };
            assert!(!resolve(&i), "{v} should disable");
        }
    }

    #[test]
    fn ci_disables() {
        assert!(!resolve(&ConsentInputs {
            is_ci: true,
            ..base()
        }));
        assert!(is_ci(Some("true")));
        assert!(!is_ci(Some("0")));
        assert!(!is_ci(None));
    }

    #[test]
    fn grace_window() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1000);
        let recent = now - Duration::from_secs(60);
        let old = now - Duration::from_secs(3600);
        assert!(within_grace(Some(recent), Duration::from_secs(600), now));
        assert!(!within_grace(Some(old), Duration::from_secs(600), now));
        assert!(!within_grace(None, Duration::from_secs(600), now));
    }
}
