use std::collections::BTreeMap;

use axum::extract::State;
use axum::{Extension, Json};
use gauge_query::{AppMeta, MetaResponse};
use sqlx::Row as _;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::error::ApiError;
use crate::middleware::bearer::AuthContext;
use crate::state::AppState;

fn db_err(e: sqlx::Error) -> ApiError {
    tracing::error!(kind = %e.to_string(), "meta db error");
    ApiError::service_unavailable("db_unavailable", "database error; retry later")
}

pub async fn meta(
    State(st): State<AppState>,
    Extension(_ctx): Extension<AuthContext>,
) -> Result<Json<MetaResponse>, ApiError> {
    let stats = sqlx::query(
        "SELECT app, COUNT(*) AS total, MIN(time) AS first, MAX(time) AS last FROM events GROUP BY app ORDER BY app",
    )
    .fetch_all(&st.pool)
    .await
    .map_err(db_err)?;
    let names = sqlx::query("SELECT DISTINCT app, event_name FROM events ORDER BY app, event_name")
        .fetch_all(&st.pool)
        .await
        .map_err(db_err)?;
    let keys = sqlx::query(
        "SELECT DISTINCT app, jsonb_object_keys(attributes) AS key FROM events ORDER BY 1, 2",
    )
    .fetch_all(&st.pool)
    .await
    .map_err(db_err)?;
    let numeric_keys = sqlx::query(
        "SELECT DISTINCT app, e.key AS key \
         FROM events, jsonb_each(attributes) AS e(key, value) \
         WHERE jsonb_typeof(e.value) = 'number' ORDER BY 1, 2",
    )
    .fetch_all(&st.pool)
    .await
    .map_err(db_err)?;

    let mut apps: BTreeMap<String, AppMeta> = BTreeMap::new();
    for row in &stats {
        let app: String = row.get("app");
        let fmt = |t: Option<OffsetDateTime>| t.and_then(|t| t.format(&Rfc3339).ok());
        apps.insert(
            app.clone(),
            AppMeta {
                app,
                event_names: vec![],
                attribute_keys: vec![],
                numeric_attribute_keys: vec![],
                first_event: fmt(row.get("first")),
                last_event: fmt(row.get("last")),
                total_events: row.get("total"),
            },
        );
    }
    for row in &names {
        let app: String = row.get("app");
        if let Some(m) = apps.get_mut(&app) {
            m.event_names.push(row.get("event_name"));
        }
    }
    for row in &keys {
        let app: String = row.get("app");
        if let Some(m) = apps.get_mut(&app) {
            m.attribute_keys.push(row.get("key"));
        }
    }
    for row in &numeric_keys {
        let app: String = row.get("app");
        if let Some(m) = apps.get_mut(&app) {
            m.numeric_attribute_keys.push(row.get("key"));
        }
    }
    Ok(Json(MetaResponse {
        apps: apps.into_values().collect(),
    }))
}
