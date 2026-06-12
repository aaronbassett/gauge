//! Tome-style doc pinning: the worked example in SPEC.md must be byte-for-byte
//! identical to the fixture that the validation tests exercise.

const SPEC: &str = include_str!("../SPEC.md");
const FIXTURE: &str = include_str!("fixtures/valid_batch.json");

#[test]
fn spec_worked_example_matches_fixture_exactly() {
    assert!(
        SPEC.contains(FIXTURE.trim()),
        "SPEC.md worked example has drifted from tests/fixtures/valid_batch.json"
    );
}

#[test]
fn spec_example_is_a_valid_gauge_batch() {
    let req: gauge_events::otlp::ExportLogsServiceRequest = serde_json::from_str(FIXTURE).unwrap();
    let batch = gauge_events::profile::validate_batch(&req, &["tome".to_string()]).unwrap();
    assert!(batch.rejections.is_empty());
}
