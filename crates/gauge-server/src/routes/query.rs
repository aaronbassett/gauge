use axum::body::Bytes;
use axum::extract::State;
use axum::{Extension, Json};
use gauge_query::{QueryRequest, QueryResponse};
use serde_json::Value;
use sqlx::Row as _;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::error::ApiError;
use crate::middleware::bearer::AuthContext;
use crate::sqlbuild::{self, Bind, ColKind};
use crate::state::AppState;

fn db_unavailable(e: sqlx::Error) -> ApiError {
    tracing::error!(kind = %e.to_string(), "query db error");
    ApiError::service_unavailable("db_unavailable", "database error; retry later")
}

pub async fn query(
    State(st): State<AppState>,
    Extension(_ctx): Extension<AuthContext>,
    body: Bytes,
) -> Result<Json<QueryResponse>, ApiError> {
    let req: QueryRequest = serde_json::from_slice(&body).map_err(|e| {
        ApiError::unprocessable("invalid_query", format!("invalid query request: {e}"))
    })?;
    let started = std::time::Instant::now();
    let built = sqlbuild::build(&req, OffsetDateTime::now_utc())
        .map_err(|e| ApiError::unprocessable("invalid_query", e.to_string()))?;

    let mut tx = st.pool.begin().await.map_err(db_unavailable)?;
    sqlx::query("SET TRANSACTION READ ONLY")
        .execute(&mut *tx)
        .await
        .map_err(db_unavailable)?;
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut *tx)
        .await
        .map_err(db_unavailable)?;

    let mut q = sqlx::query(&built.sql);
    for b in &built.binds {
        q = match b {
            Bind::Text(s) => q.bind(s),
            Bind::TextArr(v) => q.bind(v),
            Bind::Time(t) => q.bind(*t),
            Bind::Float(f) => q.bind(*f),
            Bind::FloatArr(v) => q.bind(v),
        };
    }
    let rows = q.fetch_all(&mut *tx).await.map_err(|e| {
        if e.to_string().contains("statement timeout") {
            ApiError::unprocessable("query_timeout", "query exceeded the 5s statement timeout")
                .with_remediation("narrow the time range or reduce dimensions")
        } else {
            db_unavailable(e)
        }
    })?;
    drop(tx); // read-only; rollback-on-drop is fine

    let truncated = rows.len() > built.limit;
    let mut out = Vec::with_capacity(rows.len().min(built.limit));
    for row in rows.iter().take(built.limit) {
        let mut obj = serde_json::Map::new();
        for col in &built.columns {
            let v = match &col.kind {
                ColKind::Text => row
                    .try_get::<Option<String>, _>(col.alias.as_str())
                    .map(|o| o.map(Value::String).unwrap_or(Value::Null)),
                ColKind::Int => row
                    .try_get::<i64, _>(col.alias.as_str())
                    .map(|n| Value::Number(n.into())),
                ColKind::TimeBucket => row
                    .try_get::<OffsetDateTime, _>(col.alias.as_str())
                    .map(|t| Value::String(t.format(&Rfc3339).unwrap_or_default())),
                ColKind::Float => row
                    .try_get::<Option<f64>, _>(col.alias.as_str())
                    .map(sqlbuild::float_value),
                ColKind::Bucket { labels } => row
                    .try_get::<Option<i32>, _>(col.alias.as_str())
                    .map(|i| sqlbuild::bucket_value(labels, i)),
            }
            .map_err(|_| {
                ApiError::service_unavailable("row_decode", "failed to decode result row")
            })?;
            obj.insert(col.alias.clone(), v);
        }
        out.push(Value::Object(obj));
    }
    let meta = if built.bucket_meta.is_empty() {
        None
    } else {
        Some(gauge_query::QueryMeta {
            buckets: built.bucket_meta.clone(),
        })
    };
    Ok(Json(QueryResponse {
        rows: out,
        truncated,
        elapsed_ms: started.elapsed().as_millis() as u64,
        meta,
    }))
}
