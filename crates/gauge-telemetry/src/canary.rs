//! A reusable canary harness. Given an event instance, it asserts that no
//! string attribute contains a forbidden substring. Apps point it at their own
//! `Event` types — running both clean instances (which must pass) and
//! deliberately leak-probed instances (a forbidden marker stuffed into each
//! free-string field, which must be caught). This is the privacy backstop for
//! Approach A (serde-typed events), where a `String` field still compiles and
//! so the structural validator alone cannot prevent free-form/PII leakage.

use serde_json::Value;

use crate::event::{Event, to_attributes};

/// A small default corpus of substrings that must never appear in any attribute
/// value. Apps SHOULD extend this with their own sensitive markers.
///
/// # Limitations
///
/// This is a marker-based denylist: it catches a value that *contains* a known
/// sensitive token, but it cannot catch a high-cardinality free-form string that
/// happens to contain no marker — a bare username, a hostname/IP, or an arbitrary
/// model / `accel` identifier. Fields that carry app-supplied free-form text
/// (e.g. `EnvAttributes::accel`, model ids) are exactly where leakage hides;
/// constrain them to a closed vocabulary, or extend this corpus with
/// field-specific probes, rather than relying on the defaults alone.
pub const FORBIDDEN_SUBSTRINGS: &[&str] = &[
    "@", "/Users/", "/home/", "C:\\", "http://", "https://", "SELECT ", "password", "secret",
];

/// Assert that no string attribute on `event` contains any `forbidden` substring.
/// Panics (test-only use) with the offending attribute and substring.
pub fn assert_no_forbidden<E: Event>(event: &E, forbidden: &[&str]) {
    let attrs = to_attributes(event).expect("event must serialize to scalar attributes");
    for (key, value) in &attrs {
        if let Value::String(s) = value {
            for f in forbidden {
                assert!(
                    !s.contains(f),
                    "attribute `{key}` = {s:?} leaked forbidden substring {f:?}"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{CommandInvoked, Outcome, Surface};

    #[test]
    fn clean_event_passes() {
        let e = CommandInvoked {
            command: "search".into(),
            duration_ms: 1,
            outcome: Outcome::Ok,
            surface: Surface::Cli,
        };
        assert_no_forbidden(&e, FORBIDDEN_SUBSTRINGS);
    }

    #[test]
    #[should_panic(expected = "leaked forbidden substring")]
    fn leaky_event_is_caught() {
        let e = CommandInvoked {
            command: "/Users/alice/secret".into(),
            duration_ms: 1,
            outcome: Outcome::Ok,
            surface: Surface::Cli,
        };
        assert_no_forbidden(&e, FORBIDDEN_SUBSTRINGS);
    }
}
