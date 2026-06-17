//! Probes every common event by checking that clean values produce no forbidden
//! substrings, proving the structural conversion carries no nested data and the
//! harness works end-to-end. (Closed-enum and numeric fields cannot carry
//! forbidden strings by construction.)

use gauge_telemetry::canary::{FORBIDDEN_SUBSTRINGS, assert_no_forbidden};
use gauge_telemetry::common::{
    CommandInvoked, ErrorEvent, Heartbeat, Install, Outcome, Surface, ToolCall,
};
use gauge_telemetry::env::EnvAttributes;

#[test]
fn common_events_with_clean_values_pass() {
    assert_no_forbidden(
        &CommandInvoked {
            command: "search".into(),
            duration_ms: 1,
            outcome: Outcome::Ok,
            surface: Surface::Cli,
        },
        FORBIDDEN_SUBSTRINGS,
    );
    assert_no_forbidden(
        &ToolCall {
            tool: "search".into(),
            latency_ms: 1,
            result_count: 3,
            outcome: Outcome::Ok,
        },
        FORBIDDEN_SUBSTRINGS,
    );
    assert_no_forbidden(
        &ErrorEvent {
            error_class: "timeout".into(),
            surface: Surface::Mcp,
        },
        FORBIDDEN_SUBSTRINGS,
    );
    assert_no_forbidden(
        &Install {
            install_method: "brew".into(),
            env: EnvAttributes::default(),
        },
        FORBIDDEN_SUBSTRINGS,
    );
    assert_no_forbidden(
        &Heartbeat {
            env: EnvAttributes {
                accel: Some("metal".into()),
                ..Default::default()
            },
        },
        FORBIDDEN_SUBSTRINGS,
    );
}
