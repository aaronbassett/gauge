use gauge_events::profile::{ParsedEvent, ResourceInfo};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

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

/// A fully-specified event row (heterogeneous resource per row), used by the
/// demo data generator. Unlike [`insert_events`], each row carries its own
/// resource attributes.
#[derive(Debug, Clone)]
pub struct GeneratedRow {
    pub app: String,
    pub app_version: String,
    pub install_id: Uuid,
    pub session_id: Uuid,
    pub os: String,
    pub arch: String,
    pub event_name: String,
    pub time: OffsetDateTime,
    pub attributes: serde_json::Value,
}

/// Bulk-insert heterogeneous rows via UNNEST, chunked to keep statements small.
/// Same parameterized shape as [`insert_events`] — values are never spliced.
pub async fn insert_generated(pool: &PgPool, rows: &[GeneratedRow]) -> Result<(), sqlx::Error> {
    for chunk in rows.chunks(1000) {
        let mut apps = Vec::with_capacity(chunk.len());
        let mut versions = Vec::with_capacity(chunk.len());
        let mut installs = Vec::with_capacity(chunk.len());
        let mut sessions = Vec::with_capacity(chunk.len());
        let mut oses = Vec::with_capacity(chunk.len());
        let mut arches = Vec::with_capacity(chunk.len());
        let mut names = Vec::with_capacity(chunk.len());
        let mut times = Vec::with_capacity(chunk.len());
        let mut attrs = Vec::with_capacity(chunk.len());
        for r in chunk {
            apps.push(r.app.clone());
            versions.push(r.app_version.clone());
            installs.push(r.install_id);
            sessions.push(r.session_id);
            oses.push(r.os.clone());
            arches.push(r.arch.clone());
            names.push(r.event_name.clone());
            times.push(r.time);
            attrs.push(r.attributes.clone());
        }
        sqlx::query(
            r#"INSERT INTO events (app, app_version, install_id, session_id, os, arch, event_name, time, attributes)
               SELECT * FROM UNNEST($1::text[], $2::text[], $3::uuid[], $4::uuid[], $5::text[], $6::text[], $7::text[], $8::timestamptz[], $9::jsonb[])"#,
        )
        .bind(&apps)
        .bind(&versions)
        .bind(&installs)
        .bind(&sessions)
        .bind(&oses)
        .bind(&arches)
        .bind(&names)
        .bind(&times)
        .bind(&attrs)
        .execute(pool)
        .await?;
    }
    Ok(())
}
