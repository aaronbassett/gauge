use gauge_query::{
    AppMeta, Dimension, Dir, Field, Granularity, Measure, Order, QueryRequest, TimeRange,
};
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
            dimensions: vec![Dimension::Field(Field::App)],
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
            dimensions: vec![Dimension::Field(Field::App)],
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
            dimensions: vec![Dimension::Field(Field::EventName)],
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

/// Round x up to a "nice" 1/2/5×10ⁿ step (for readable histogram edges).
fn nice_round(x: f64) -> f64 {
    if x <= 0.0 {
        return 1.0;
    }
    let mag = 10f64.powf(x.log10().floor());
    let norm = x / mag; // 1.0..10.0
    let nice = if norm < 1.5 {
        1.0
    } else if norm < 3.0 {
        2.0
    } else if norm < 7.0 {
        5.0
    } else {
        10.0
    };
    nice * mag
}

/// ~6 nicely-rounded interior edges spanning [min, max] (→ up to 5 edges → 6 buckets).
pub fn derive_edges(min: f64, max: f64) -> Vec<f64> {
    if !min.is_finite() || !max.is_finite() || max <= min {
        return vec![nice_round(min.abs().max(1.0))];
    }
    let step = nice_round((max - min) / 6.0);
    let mut edges = Vec::new();
    let mut e = (min / step).ceil() * step;
    if e <= min {
        e += step;
    }
    while e < max && edges.len() < 5 {
        edges.push(e);
        e += step;
    }
    if edges.is_empty() {
        edges.push((min + max) / 2.0);
    }
    edges
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn derive_edges_are_sorted_rounded_and_bounded() {
        let e = derive_edges(0.0, 1180.0);
        assert!(!e.is_empty() && e.len() <= 5);
        assert!(
            e.windows(2).all(|w| w[0] < w[1]),
            "edges must be strictly ascending"
        );
        // degenerate range still yields a usable single split
        let d = derive_edges(5.0, 5.0);
        assert_eq!(d.len(), 1);
    }
}
