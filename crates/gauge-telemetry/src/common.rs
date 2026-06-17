//! The shared "common event" types — the ergonomic shortcuts. App-specific
//! events stay as the app's own `Event` types. This is a proposed starting set
//! (per the design spec) and is extended additively during porting.

use std::borrow::Cow;

use serde::Serialize;

use crate::env::EnvAttributes;
use crate::event::Event;

/// Coarse outcome — union of the two apps' outcome enums.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Ok,
    Failed,
    InvalidInput,
}

/// Where the activity happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
    Cli,
    Mcp,
}

/// First run on this install. Carries the environment snapshot.
#[derive(Debug, Clone, Serialize)]
pub struct Install {
    pub install_method: String,
    #[serde(flatten)]
    pub env: EnvAttributes,
}
impl Event for Install {
    fn name(&self) -> Cow<'_, str> {
        "install".into()
    }
}

/// Periodic liveness event (cadence chosen by the app). Carries the environment.
#[derive(Debug, Clone, Serialize)]
pub struct Heartbeat {
    #[serde(flatten)]
    pub env: EnvAttributes,
}
impl Event for Heartbeat {
    fn name(&self) -> Cow<'_, str> {
        "heartbeat".into()
    }
}

/// One top-level command/subcommand completed.
#[derive(Debug, Clone, Serialize)]
pub struct CommandInvoked {
    pub command: String,
    pub duration_ms: u32,
    pub outcome: Outcome,
    pub surface: Surface,
}
impl Event for CommandInvoked {
    fn name(&self) -> Cow<'_, str> {
        "command_invoked".into()
    }
}

/// One tool/operation invocation completed.
#[derive(Debug, Clone, Serialize)]
pub struct ToolCall {
    pub tool: String,
    pub latency_ms: u32,
    pub result_count: u32,
    pub outcome: Outcome,
}
impl Event for ToolCall {
    fn name(&self) -> Cow<'_, str> {
        "tool_call".into()
    }
}

/// A failure, classified by a closed `error_class` (never a raw message).
#[derive(Debug, Clone, Serialize)]
pub struct ErrorEvent {
    pub error_class: String,
    pub surface: Surface,
}
impl Event for ErrorEvent {
    fn name(&self) -> Cow<'_, str> {
        "error".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::to_attributes;
    use serde_json::Value;

    #[test]
    fn command_invoked_serializes_to_expected_scalars() {
        let e = CommandInvoked {
            command: "search".into(),
            duration_ms: 142,
            outcome: Outcome::Ok,
            surface: Surface::Cli,
        };
        assert_eq!(e.name(), "command_invoked");
        let a = to_attributes(&e).unwrap();
        assert_eq!(a["command"], Value::String("search".into()));
        assert_eq!(a["duration_ms"], Value::Number(142u32.into()));
        assert_eq!(a["outcome"], Value::String("ok".into()));
        assert_eq!(a["surface"], Value::String("cli".into()));
    }

    #[test]
    fn heartbeat_flattens_env_and_omits_missing() {
        let e = Heartbeat {
            env: EnvAttributes {
                cpu_cores: Some(8),
                accel: Some("metal".into()),
                ..Default::default()
            },
        };
        let a = to_attributes(&e).unwrap();
        assert_eq!(a["cpu_cores"], Value::Number(8u32.into()));
        assert_eq!(a["accel"], Value::String("metal".into()));
        assert!(!a.contains_key("ram_gb")); // None omitted, no nested object
    }
}
