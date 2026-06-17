//! End-to-end (no network): emit through the kernel, read the queue the way the
//! sender does, re-encode the OTLP batch, and prove it passes the Gauge profile
//! validator the server uses on ingest.

use gauge_events::profile::validate_batch;
use gauge_events::sender::queue::read_lines;
use gauge_events::sender::{QueuedEvent, SenderConfig, encode_batch};
use gauge_telemetry::Telemetry;
use gauge_telemetry::common::{CommandInvoked, Heartbeat, Outcome, Surface};
use gauge_telemetry::env::EnvAttributes;

#[test]
fn emitted_events_pass_the_gauge_profile() {
    let tmp = tempfile::tempdir().unwrap();
    let t = Telemetry::builder()
        .app("tome")
        .app_version("0.7.0")
        .endpoint("https://example.invalid")
        .install_id_path(tmp.path().join("id"))
        .config_enabled(true)
        .runtime_enabled(true)
        .ci(false)
        .build()
        .unwrap();

    t.emit(&CommandInvoked {
        command: "search".into(),
        duration_ms: 142,
        outcome: Outcome::Ok,
        surface: Surface::Cli,
    });
    t.emit(&Heartbeat {
        env: EnvAttributes {
            cpu_cores: Some(8),
            ram_gb: Some(16),
            accel: Some("metal".into()),
            ..Default::default()
        },
    });

    // Rebuild the batch the same way `drain` does.
    let queue = tmp.path().join("id.queue.jsonl");
    let events: Vec<QueuedEvent> = read_lines(&queue)
        .unwrap()
        .iter()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(events.len(), 2);

    // A SenderConfig identical in resource shape to the live one.
    let cfg = SenderConfig {
        endpoint: "https://example.invalid".into(),
        app: "tome".into(),
        app_version: "0.7.0".into(),
        install_id: uuid::Uuid::new_v4(),
        session_id: uuid::Uuid::new_v4(),
        os: gauge_telemetry::env::os_type(),
        arch: gauge_telemetry::env::host_arch(),
        queue_path: queue.clone(),
    };
    let req = encode_batch(&cfg, &events);

    let batch = validate_batch(&req, &["tome".to_string()]).expect("must validate");
    assert_eq!(batch.resource.app, "tome");
    assert_eq!(batch.events.len(), 2);
    assert!(
        batch.rejections.is_empty(),
        "no rejections: {:?}",
        batch.rejections
    );
    assert!(
        batch
            .events
            .iter()
            .all(|e| e.event_name.starts_with("tome."))
    );
}
