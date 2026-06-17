//! Pins the exact OTLP body for the SPEC.md worked example. If this snapshot
//! changes, the wire contract changed — update SPEC.md deliberately.

use gauge_events::sender::{QueuedEvent, SenderConfig, encode_batch};

fn fixed_cfg(queue: std::path::PathBuf) -> SenderConfig {
    SenderConfig {
        endpoint: "https://example.invalid".into(),
        app: "tome".into(),
        app_version: "0.7.0".into(),
        install_id: uuid::Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap(),
        session_id: uuid::Uuid::parse_str("00000000-0000-4000-8000-000000000002").unwrap(),
        os: "darwin".into(),
        arch: "arm64".into(),
        queue_path: queue,
    }
}

#[test]
fn command_invoked_otlp_body_is_pinned() {
    // The exact attributes `to_attributes(CommandInvoked{..})` produces.
    let mut attributes = serde_json::Map::new();
    attributes.insert("command".into(), serde_json::json!("search"));
    attributes.insert("duration_ms".into(), serde_json::json!(142));
    attributes.insert("outcome".into(), serde_json::json!("ok"));
    attributes.insert("surface".into(), serde_json::json!("cli"));

    let ev = QueuedEvent {
        event_name: "tome.command_invoked".into(),
        time_unix_nano: 1_781_430_705_123_000_000,
        attributes,
    };
    let tmp = tempfile::tempdir().unwrap();
    let req = encode_batch(&fixed_cfg(tmp.path().join("q.jsonl")), &[ev]);
    let body = serde_json::to_string_pretty(&req).unwrap();
    insta::assert_snapshot!(body);
}
