use gauge_query::{AppMeta, Dir, Field, Granularity, Measure, Order, QueryRequest, TimeRange};
use time::OffsetDateTime;

use crate::api::ApiClient;
use crate::error::ClientError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeWindow {
    H1,
    H24,
    D7,
    D30,
}

impl TimeWindow {
    pub fn next(self) -> Self {
        match self {
            Self::H1 => Self::H24,
            Self::H24 => Self::D7,
            Self::D7 => Self::D30,
            Self::D30 => Self::H1,
        }
    }
    pub fn last(&self) -> &'static str {
        match self {
            Self::H1 => "1h",
            Self::H24 => "24h",
            Self::D7 => "7d",
            Self::D30 => "30d",
        }
    }
    pub fn granularity(&self) -> Granularity {
        match self {
            Self::H1 | Self::H24 => Granularity::Hour,
            Self::D7 | Self::D30 => Granularity::Day,
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            Self::H1 => "last hour",
            Self::H24 => "last 24h",
            Self::D7 => "last 7d",
            Self::D30 => "last 30d",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub fetched_at: OffsetDateTime,
    pub window: TimeWindow,
    /// rows: {time_bucket, app, count}
    pub timeseries: Vec<serde_json::Value>,
    /// rows: {app, count, unique_installs, unique_sessions}
    pub totals: Vec<serde_json::Value>,
    /// rows: {event_name, count}
    pub top_events: Vec<serde_json::Value>,
    pub apps: Vec<AppMeta>,
}

fn base(w: TimeWindow) -> QueryRequest {
    QueryRequest {
        measures: vec![Measure::Count],
        dimensions: vec![],
        filters: vec![],
        time_range: TimeRange::Last {
            last: w.last().into(),
        },
        granularity: None,
        order: vec![],
        limit: None,
    }
}

pub async fn fetch(api: &ApiClient, w: TimeWindow) -> Result<Snapshot, ClientError> {
    let timeseries = api
        .query(&QueryRequest {
            dimensions: vec![Field::App],
            granularity: Some(w.granularity()),
            ..base(w)
        })
        .await?
        .rows;
    let totals = api
        .query(&QueryRequest {
            measures: vec![
                Measure::Count,
                Measure::UniqueInstalls,
                Measure::UniqueSessions,
            ],
            dimensions: vec![Field::App],
            order: vec![Order {
                field: "app".into(),
                dir: Dir::Asc,
            }],
            ..base(w)
        })
        .await?
        .rows;
    let top_events = api
        .query(&QueryRequest {
            dimensions: vec![Field::EventName],
            order: vec![Order {
                field: "count".into(),
                dir: Dir::Desc,
            }],
            limit: Some(10),
            ..base(w)
        })
        .await?
        .rows;
    let apps = api.meta().await?.apps;
    Ok(Snapshot {
        fetched_at: OffsetDateTime::now_utc(),
        window: w,
        timeseries,
        totals,
        top_events,
        apps,
    })
}
