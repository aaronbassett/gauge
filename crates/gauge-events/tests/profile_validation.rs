use gauge_events::otlp::ExportLogsServiceRequest;
use gauge_events::profile::{BatchError, validate_batch};

const FIXTURE: &str = include_str!("fixtures/valid_batch.json");

fn fixture() -> ExportLogsServiceRequest {
    serde_json::from_str(FIXTURE).unwrap()
}

fn allow() -> Vec<String> {
    vec!["tome".to_string(), "midnight-manual".to_string()]
}

#[test]
fn valid_batch_passes() {
    let batch = validate_batch(&fixture(), &allow()).unwrap();
    assert_eq!(batch.resource.app, "tome");
    assert_eq!(batch.resource.os, "darwin");
    assert_eq!(batch.events.len(), 1);
    assert!(batch.rejections.is_empty());
    let ev = &batch.events[0];
    assert_eq!(ev.event_name, "tome.search");
    // event.name attribute is stripped from stored attributes
    assert!(!ev.attributes.contains_key("event.name"));
    assert_eq!(ev.attributes["surface"], serde_json::json!("cli"));
    assert_eq!(ev.attributes["reranker_used"], serde_json::json!(true));
    assert_eq!(ev.attributes["candidates_returned"], serde_json::json!(12));
}

#[test]
fn unknown_app_is_batch_error() {
    let err = validate_batch(&fixture(), &["other".to_string()]).unwrap_err();
    assert!(matches!(err, BatchError::UnknownApp(a) if a == "tome"));
}

#[test]
fn missing_resource_attr_is_batch_error() {
    let mut req = fixture();
    req.resource_logs[0]
        .resource
        .as_mut()
        .unwrap()
        .attributes
        .retain(|kv| kv.key != "service.instance.id");
    let err = validate_batch(&req, &allow()).unwrap_err();
    assert!(matches!(err, BatchError::BadResourceAttr("service.instance.id")));
}

#[test]
fn bad_os_type_is_batch_error() {
    let mut req = fixture();
    for kv in &mut req.resource_logs[0].resource.as_mut().unwrap().attributes {
        if kv.key == "os.type" {
            kv.value.string_value = Some("macos".into()); // profile requires "darwin"
        }
    }
    assert!(matches!(validate_batch(&req, &allow()), Err(BatchError::BadResourceAttr("os.type"))));
}

#[test]
fn multiple_resource_blocks_rejected() {
    let mut req = fixture();
    let dup = req.resource_logs[0].clone();
    req.resource_logs.push(dup);
    assert!(matches!(validate_batch(&req, &allow()), Err(BatchError::ExpectedSingleResource)));
}

#[test]
fn record_missing_event_name_is_rejected_not_fatal() {
    let mut req = fixture();
    let mut bad = req.resource_logs[0].scope_logs[0].log_records[0].clone();
    bad.event_name = None;
    bad.attributes.retain(|kv| kv.key != "event.name");
    req.resource_logs[0].scope_logs[0].log_records.push(bad);
    let batch = validate_batch(&req, &allow()).unwrap();
    assert_eq!(batch.events.len(), 1);
    assert_eq!(batch.rejections.len(), 1);
    assert_eq!(batch.rejections[0].index, 1);
    assert!(batch.rejections[0].reason.contains("event name"));
}

#[test]
fn event_name_falls_back_to_attribute() {
    let mut req = fixture();
    req.resource_logs[0].scope_logs[0].log_records[0].event_name = None;
    let batch = validate_batch(&req, &allow()).unwrap();
    assert_eq!(batch.events[0].event_name, "tome.search");
}

#[test]
fn wrong_prefix_rejected() {
    let mut req = fixture();
    req.resource_logs[0].scope_logs[0].log_records[0].event_name = Some("other.search".into());
    let batch = validate_batch(&req, &allow()).unwrap();
    assert_eq!(batch.rejections.len(), 1);
    assert!(batch.rejections[0].reason.contains("prefixed"));
}

#[test]
fn oversized_attribute_string_rejected() {
    let mut req = fixture();
    req.resource_logs[0].scope_logs[0].log_records[0]
        .attributes
        .push(gauge_events::otlp::KeyValue {
            key: "big".into(),
            value: gauge_events::otlp::AnyValue {
                string_value: Some("x".repeat(129)),
                ..Default::default()
            },
        });
    let batch = validate_batch(&req, &allow()).unwrap();
    assert_eq!(batch.rejections.len(), 1);
    assert!(batch.rejections[0].reason.contains("128"));
}

#[test]
fn too_many_records_is_batch_error() {
    let mut req = fixture();
    let rec = req.resource_logs[0].scope_logs[0].log_records[0].clone();
    req.resource_logs[0].scope_logs[0].log_records = vec![rec; 1001];
    assert!(matches!(validate_batch(&req, &allow()), Err(BatchError::TooManyRecords)));
}
