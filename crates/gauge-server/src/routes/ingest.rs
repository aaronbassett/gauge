use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use gauge_events::otlp::{
    ExportLogsPartialSuccess, ExportLogsServiceRequest, ExportLogsServiceResponse,
};
use gauge_events::profile::validate_batch;

use crate::db;
use crate::error::ApiError;
use crate::state::AppState;

/// Anonymous OTLP/HTTP logs ingest. Privacy rules for this handler:
/// never log request bodies, attribute values, or client IPs.
pub async fn ingest(
    State(st): State<AppState>,
    body: Bytes,
) -> Result<Json<ExportLogsServiceResponse>, ApiError> {
    let req: ExportLogsServiceRequest = serde_json::from_slice(&body).map_err(|_| {
        ApiError::bad_request(
            "invalid_otlp",
            "request body is not valid OTLP/HTTP JSON (logs signal)",
        )
    })?;

    let batch = validate_batch(&req, &st.allowlist)
        .map_err(|e| ApiError::bad_request("invalid_batch", e.to_string()))?;

    if !batch.events.is_empty() {
        db::insert_events(&st.pool, &batch.resource, &batch.events)
            .await
            .map_err(|e| {
                tracing::error!(kind = %e.to_string(), "ingest insert failed");
                ApiError::service_unavailable(
                    "db_unavailable",
                    "could not persist events; retry later",
                )
            })?;
    }

    tracing::info!(
        app = %batch.resource.app,
        accepted = batch.events.len(),
        rejected = batch.rejections.len(),
        "ingest"
    );

    let partial_success = (!batch.rejections.is_empty()).then(|| ExportLogsPartialSuccess {
        rejected_log_records: batch.rejections.len() as i64,
        error_message: batch
            .rejections
            .iter()
            .take(5)
            .map(|r| format!("record {}: {}", r.index, r.reason))
            .collect::<Vec<_>>()
            .join("; "),
    });
    Ok(Json(ExportLogsServiceResponse { partial_success }))
}
