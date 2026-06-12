use gauge_events::otlp::ExportLogsServiceRequest;

const FIXTURE: &str = include_str!("fixtures/valid_batch.json");

#[test]
fn fixture_parses() {
    let req: ExportLogsServiceRequest = serde_json::from_str(FIXTURE).unwrap();
    let rl = &req.resource_logs[0];
    assert_eq!(rl.resource.as_ref().unwrap().attributes.len(), 6);
    let rec = &rl.scope_logs[0].log_records[0];
    assert_eq!(rec.event_name.as_deref(), Some("tome.search"));
    assert_eq!(rec.time_unix_nano, Some(1_781_430_705_123_000_000));
    assert_eq!(rec.attributes.len(), 5);
}

#[test]
fn serialization_round_trips() {
    let req: ExportLogsServiceRequest = serde_json::from_str(FIXTURE).unwrap();
    let json = serde_json::to_string(&req).unwrap();
    let back: ExportLogsServiceRequest = serde_json::from_str(&json).unwrap();
    // timeUnixNano must serialize back to a string (protobuf JSON int64 rule)
    assert!(json.contains("\"timeUnixNano\":\"1781430705123000000\""));
    assert_eq!(back.resource_logs[0].scope_logs[0].log_records[0].time_unix_nano,
               Some(1_781_430_705_123_000_000));
}

#[test]
fn time_unix_nano_accepts_json_number_too() {
    let req: ExportLogsServiceRequest =
        serde_json::from_str(r#"{"resourceLogs":[{"scopeLogs":[{"logRecords":[{"timeUnixNano":123}]}]}]}"#).unwrap();
    assert_eq!(req.resource_logs[0].scope_logs[0].log_records[0].time_unix_nano, Some(123));
}
