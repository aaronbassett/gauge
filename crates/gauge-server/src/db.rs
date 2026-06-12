use gauge_events::profile::{ParsedEvent, ResourceInfo};
use sqlx::PgPool;
use time::OffsetDateTime;

/// Batch insert via UNNEST: one statement regardless of batch size,
/// fully parameterized.
pub async fn insert_events(
    pool: &PgPool,
    res: &ResourceInfo,
    events: &[ParsedEvent],
) -> Result<(), sqlx::Error> {
    if events.is_empty() {
        return Ok(());
    }
    let mut names: Vec<String> = Vec::with_capacity(events.len());
    let mut times: Vec<OffsetDateTime> = Vec::with_capacity(events.len());
    let mut attrs: Vec<serde_json::Value> = Vec::with_capacity(events.len());
    for e in events {
        names.push(e.event_name.clone());
        times.push(e.time);
        attrs.push(serde_json::Value::Object(e.attributes.clone()));
    }
    sqlx::query(
        r#"INSERT INTO events (app, app_version, install_id, session_id, os, arch, event_name, time, attributes)
           SELECT $1, $2, $3, $4, $5, $6, n, t, a
           FROM UNNEST($7::text[], $8::timestamptz[], $9::jsonb[]) AS u(n, t, a)"#,
    )
    .bind(&res.app)
    .bind(&res.app_version)
    .bind(res.install_id)
    .bind(res.session_id)
    .bind(&res.os)
    .bind(&res.arch)
    .bind(&names)
    .bind(&times)
    .bind(&attrs)
    .execute(pool)
    .await?;
    Ok(())
}
