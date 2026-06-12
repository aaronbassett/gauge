use std::path::PathBuf;

use serde_json::{Map, Value};
use uuid::Uuid;

use crate::otlp::{
    AnyValue, ExportLogsServiceRequest, KeyValue, LogRecord, Resource, ResourceLogs, ScopeLogs,
};
use crate::sender::queue::{self, AppendOutcome};

#[derive(Debug, Clone)]
pub struct SenderConfig {
    /// Base server URL (no trailing slash), e.g. https://gauge-telemetry.fly.dev
    pub endpoint: String,
    pub app: String,
    pub app_version: String,
    pub install_id: Uuid,
    pub session_id: Uuid,
    pub os: String,
    pub arch: String,
    pub queue_path: PathBuf,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct QueuedEvent {
    pub event_name: String,
    pub time_unix_nano: u64,
    pub attributes: Map<String, Value>,
}

/// One append, no network — safe to call from any foreground path.
pub fn enqueue(
    cfg: &SenderConfig,
    event_name: &str,
    attributes: Map<String, Value>,
) -> std::io::Result<AppendOutcome> {
    let now = time::OffsetDateTime::now_utc();
    let ev = QueuedEvent {
        event_name: event_name.to_string(),
        time_unix_nano: now.unix_timestamp_nanos().max(0) as u64,
        attributes,
    };
    let line = serde_json::to_string(&ev).expect("QueuedEvent always serializes");
    queue::append_line(&cfg.queue_path, &line)
}

fn str_kv(key: &str, v: &str) -> KeyValue {
    KeyValue {
        key: key.into(),
        value: AnyValue { string_value: Some(v.into()), ..Default::default() },
    }
}

fn value_to_any(v: &Value) -> AnyValue {
    match v {
        Value::String(s) => AnyValue { string_value: Some(s.clone()), ..Default::default() },
        Value::Bool(b) => AnyValue { bool_value: Some(*b), ..Default::default() },
        Value::Number(n) if n.is_i64() => AnyValue {
            int_value: Some(n.as_i64().unwrap_or(0).to_string()),
            ..Default::default()
        },
        Value::Number(n) => AnyValue { double_value: n.as_f64(), ..Default::default() },
        // non-scalars never come from enqueue(); encode defensively as nothing
        _ => AnyValue::default(),
    }
}

pub fn encode_batch(cfg: &SenderConfig, events: &[QueuedEvent]) -> ExportLogsServiceRequest {
    let resource = Resource {
        attributes: vec![
            str_kv("service.name", &cfg.app),
            str_kv("service.version", &cfg.app_version),
            str_kv("service.instance.id", &cfg.install_id.to_string()),
            str_kv("session.id", &cfg.session_id.to_string()),
            str_kv("os.type", &cfg.os),
            str_kv("host.arch", &cfg.arch),
        ],
    };
    let log_records = events
        .iter()
        .map(|e| {
            let mut attributes = vec![str_kv("event.name", &e.event_name)];
            attributes.extend(
                e.attributes
                    .iter()
                    .map(|(k, v)| KeyValue { key: k.clone(), value: value_to_any(v) }),
            );
            LogRecord {
                time_unix_nano: Some(e.time_unix_nano),
                event_name: Some(e.event_name.clone()),
                attributes,
            }
        })
        .collect();
    ExportLogsServiceRequest {
        resource_logs: vec![ResourceLogs {
            resource: Some(resource),
            scope_logs: vec![ScopeLogs { log_records }],
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::validate_batch;

    fn cfg(tmp: &std::path::Path) -> SenderConfig {
        SenderConfig {
            endpoint: "https://gauge-telemetry.fly.dev".into(),
            app: "tome".into(),
            app_version: "0.7.0".into(),
            install_id: uuid::Uuid::new_v4(),
            session_id: uuid::Uuid::new_v4(),
            os: "darwin".into(),
            arch: "arm64".into(),
            queue_path: tmp.join("queue.jsonl"),
        }
    }

    #[test]
    fn encoded_batch_passes_profile_validation() {
        let tmp = tempfile::tempdir().unwrap();
        let c = cfg(tmp.path());
        let mut attributes = serde_json::Map::new();
        attributes.insert("surface".into(), serde_json::json!("cli"));
        attributes.insert("reranker_used".into(), serde_json::json!(true));
        attributes.insert("candidates".into(), serde_json::json!(12));
        let ev = QueuedEvent {
            event_name: "tome.search".into(),
            time_unix_nano: 1_781_430_705_123_000_000,
            attributes,
        };
        let req = encode_batch(&c, &[ev]);
        let batch = validate_batch(&req, &["tome".to_string()]).unwrap();
        assert_eq!(batch.resource.app, "tome");
        assert_eq!(batch.events.len(), 1);
        assert!(batch.rejections.is_empty(), "{:?}", batch.rejections);
        // both event-name carriers present
        let rec = &req.resource_logs[0].scope_logs[0].log_records[0];
        assert_eq!(rec.event_name.as_deref(), Some("tome.search"));
        assert!(rec.attributes.iter().any(|kv| kv.key == "event.name"));
    }

    #[test]
    fn enqueue_writes_parseable_line() {
        let tmp = tempfile::tempdir().unwrap();
        let c = cfg(tmp.path());
        let mut attributes = serde_json::Map::new();
        attributes.insert("surface".into(), serde_json::json!("mcp"));
        enqueue(&c, "tome.search", attributes).unwrap();
        let lines = crate::sender::queue::read_lines(&c.queue_path).unwrap();
        assert_eq!(lines.len(), 1);
        let ev: QueuedEvent = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(ev.event_name, "tome.search");
        assert!(ev.time_unix_nano > 0);
    }
}
