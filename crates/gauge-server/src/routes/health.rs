use axum::extract::State;

use crate::error::ApiError;
use crate::state::AppState;

pub async fn healthz() -> &'static str {
    "ok"
}

pub async fn readyz(State(st): State<AppState>) -> Result<&'static str, ApiError> {
    sqlx::query("SELECT 1")
        .execute(&st.pool)
        .await
        .map_err(|_| ApiError::service_unavailable("db_unavailable", "database not reachable"))?;
    Ok("ok")
}
