mod common;

use std::io::Write;
use std::sync::{Arc, Mutex};

use gauge_server::app::build_router;
use sqlx::PgPool;
use tracing_subscriber::fmt::MakeWriter;

/// Canary 1: the events schema must contain exactly the spec's columns —
/// catching any accidental addition of IP/UA/identity columns in review.
#[sqlx::test(migrations = "../../migrations")]
async fn events_schema_has_exactly_the_spec_columns(pool: PgPool) {
    let cols: Vec<String> = sqlx::query_scalar(
        "SELECT column_name FROM information_schema.columns WHERE table_name = 'events' ORDER BY column_name",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        cols,
        vec![
            "app",
            "app_version",
            "arch",
            "attributes",
            "event_name",
            "id",
            "install_id",
            "os",
            "received_at",
            "session_id",
            "time",
        ],
        "events table columns drifted from the spec — privacy review required"
    );
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Capture {
    type Writer = Capture;
    fn make_writer(&'a self) -> Capture {
        self.clone()
    }
}

/// Canary 2: nothing logged on the ingest path may contain attribute values.
/// (sqlx::test uses a current-thread runtime, so the thread-local default
/// subscriber covers the handler.)
#[sqlx::test(migrations = "../../migrations")]
async fn ingest_path_never_logs_attribute_values(pool: PgPool) {
    const CANARY: &str = "SECRET_CANARY_VALUE_DO_NOT_LOG";
    let capture = Capture::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(capture.clone())
        .with_max_level(tracing::Level::TRACE)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let (state, _kp) = common::test_state(pool);
    let app = build_router(state);
    let mut body: serde_json::Value = serde_json::from_str(include_str!(
        "../../gauge-events/tests/fixtures/valid_batch.json"
    ))
    .unwrap();
    body["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0]["attributes"][1]["value"]["stringValue"] =
        CANARY.into();
    let (status, _) = common::send_json(&app, "POST", "/v1/logs", Some(body), None).await;
    assert_eq!(status, axum::http::StatusCode::OK);

    let logs = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
    // Guard against a vacuous pass: prove the subscriber actually captured the
    // ingest handler's output. The handler emits `tracing::info!(... "ingest")`,
    // so an empty/un-wired capture (which would make the leak check meaningless)
    // is caught here.
    assert!(
        logs.contains("ingest"),
        "canary is vacuous: no ingest-path log was captured, so the leak check below proves nothing:\n{logs}"
    );
    assert!(
        !logs.contains(CANARY),
        "ingest path logged an attribute value:\n{logs}"
    );
}
