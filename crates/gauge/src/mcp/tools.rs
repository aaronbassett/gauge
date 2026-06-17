//! Pure parameter→QueryRequest builders for the MCP convenience tools.
//! Separated from rmcp glue so they unit-test without a server.

use gauge_query::{
    BucketSpec, Dimension, Dir, Field, Filter, FilterOp, FilterValue, Granularity, Measure, Order,
    QueryRequest, TimeRange,
};
use schemars::JsonSchema;
use serde::Deserialize;

fn eq_filter(field: Field, value: &str) -> Filter {
    Filter {
        field,
        op: FilterOp::Eq,
        value: Some(FilterValue::One(value.to_string())),
    }
}

fn base_filters(app: &Option<String>, event_name: &Option<String>) -> Vec<Filter> {
    let mut f = Vec::new();
    if let Some(a) = app {
        f.push(eq_filter(Field::App, a));
    }
    if let Some(e) = event_name {
        f.push(eq_filter(Field::EventName, e));
    }
    f
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UniqueUsersParams {
    /// Relative look-back window, e.g. "24h", "7d", "30d".
    pub period: String,
    /// Restrict to one app (a value from get_meta's `apps[].app`). Omit for all apps.
    pub app: Option<String>,
    /// Restrict to one event name (from get_meta's `apps[].event_names`). Omit for all events.
    pub event_name: Option<String>,
}

pub fn unique_users_query(p: &UniqueUsersParams) -> QueryRequest {
    QueryRequest {
        measures: vec![Measure::UniqueInstalls],
        dimensions: vec![],
        filters: base_filters(&p.app, &p.event_name),
        time_range: TimeRange::Last {
            last: p.period.clone(),
        },
        granularity: None,
        order: vec![],
        limit: None,
    }
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TopBy {
    Count,
    UniqueInstalls,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TopEventsParams {
    /// Relative look-back window, e.g. "24h", "7d", "30d".
    pub period: String,
    /// Restrict to one app (a value from get_meta's `apps[].app`). Omit for all apps.
    pub app: Option<String>,
    /// Rank by total count (default) or by unique installs.
    pub by: Option<TopBy>,
    /// Max number of events to return (default 10).
    pub limit: Option<u32>,
}

pub fn top_events_query(p: &TopEventsParams) -> QueryRequest {
    let measure = match p.by.unwrap_or(TopBy::Count) {
        TopBy::Count => Measure::Count,
        TopBy::UniqueInstalls => Measure::UniqueInstalls,
    };
    let order_field = measure.alias();
    QueryRequest {
        measures: vec![measure],
        dimensions: vec![Dimension::Field(Field::EventName)],
        filters: base_filters(&p.app, &None),
        time_range: TimeRange::Last {
            last: p.period.clone(),
        },
        granularity: None,
        order: vec![Order {
            field: order_field,
            dir: Dir::Desc,
        }],
        limit: Some(p.limit.unwrap_or(10)),
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EventsOverTimeParams {
    /// Relative look-back window, e.g. "24h", "7d", "30d".
    pub period: String,
    /// Time-bucket size for the series: hour, day, or week.
    pub granularity: Granularity,
    /// Restrict to one app (a value from get_meta's `apps[].app`). Omit for all apps.
    pub app: Option<String>,
    /// Restrict to one event name (from get_meta's `apps[].event_names`). Omit for all events.
    pub event_name: Option<String>,
}

pub fn events_over_time_query(p: &EventsOverTimeParams) -> QueryRequest {
    QueryRequest {
        measures: vec![Measure::Count],
        dimensions: vec![],
        filters: base_filters(&p.app, &p.event_name),
        time_range: TimeRange::Last {
            last: p.period.clone(),
        },
        granularity: Some(p.granularity),
        order: vec![],
        limit: None,
    }
}

/// A numeric attribute key — accepts a bare key ("latency_ms") or an
/// "attr."-prefixed key ("attr.latency_ms") — resolved to `Field::Attr`.
/// Errors if the key is empty or uses characters the DSL disallows.
fn attr_field(key: &str) -> Result<Field, String> {
    let bare = key.strip_prefix("attr.").unwrap_or(key);
    Field::parse(&format!("attr.{bare}")).map_err(|_| {
        format!("invalid attribute key `{key}` (allowed: letters, digits, '_', '.', max 64 chars)")
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NumericStatsParams {
    /// Relative look-back window, e.g. "24h", "7d", "30d".
    pub period: String,
    /// Numeric attribute key (from get_meta's `apps[].numeric_attribute_keys`), e.g. "latency_ms".
    pub field: String,
    /// Restrict to one app. Omit for all apps.
    pub app: Option<String>,
    /// Restrict to one event name. Omit for all events.
    pub event_name: Option<String>,
}

pub fn numeric_stats_query(p: &NumericStatsParams) -> Result<QueryRequest, String> {
    let f = attr_field(&p.field)?;
    Ok(QueryRequest {
        measures: vec![
            Measure::Avg(f.clone()),
            Measure::Min(f.clone()),
            Measure::Max(f.clone()),
            Measure::P50(f.clone()),
            Measure::P90(f.clone()),
            Measure::P95(f.clone()),
            Measure::P99(f),
        ],
        dimensions: vec![],
        filters: base_filters(&p.app, &p.event_name),
        time_range: TimeRange::Last {
            last: p.period.clone(),
        },
        granularity: None,
        order: vec![],
        limit: None,
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NumericHistogramParams {
    /// Relative look-back window, e.g. "24h", "7d", "30d".
    pub period: String,
    /// Numeric attribute key (from get_meta's `apps[].numeric_attribute_keys`), e.g. "latency_ms".
    pub field: String,
    /// Bucket edges (ascending), e.g. [50.0, 200.0, 500.0]. Produces N+1 buckets.
    pub edges: Vec<f64>,
    /// Restrict to one app. Omit for all apps.
    pub app: Option<String>,
    /// Restrict to one event name. Omit for all events.
    pub event_name: Option<String>,
}

pub fn numeric_histogram_query(p: &NumericHistogramParams) -> Result<QueryRequest, String> {
    let f = attr_field(&p.field)?;
    Ok(QueryRequest {
        measures: vec![Measure::Count, Measure::UniqueInstalls],
        dimensions: vec![Dimension::Bucket {
            bucket: BucketSpec {
                field: f,
                edges: p.edges.clone(),
            },
        }],
        filters: base_filters(&p.app, &p.event_name),
        time_range: TimeRange::Last {
            last: p.period.clone(),
        },
        granularity: None,
        order: vec![],
        limit: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_stats_builds_all_aggregates() {
        let q = numeric_stats_query(&NumericStatsParams {
            period: "7d".into(),
            field: "latency_ms".into(),
            app: Some("tome".into()),
            event_name: None,
        })
        .unwrap();
        // avg/min/max + four percentiles over attr.latency_ms
        assert_eq!(q.measures.len(), 7);
        assert!(
            q.measures
                .iter()
                .any(|m| matches!(m, Measure::P95(f) if f.to_string() == "attr.latency_ms"))
        );
        gauge_query::validate(&q).unwrap();
    }

    #[test]
    fn numeric_stats_query_with_event_name_has_two_filters() {
        let q = numeric_stats_query(&NumericStatsParams {
            period: "7d".into(),
            field: "latency_ms".into(),
            app: Some("tome".into()),
            event_name: Some("tome.search".into()),
        })
        .unwrap();
        // Must include both an app filter and an event_name filter.
        assert_eq!(
            q.filters.len(),
            2,
            "expected 2 filters (app + event_name), got {}",
            q.filters.len()
        );
        assert!(
            q.filters.iter().any(|f| f.field == Field::App),
            "missing app filter"
        );
        assert!(
            q.filters.iter().any(|f| f.field == Field::EventName),
            "missing event_name filter"
        );
        gauge_query::validate(&q).unwrap();
    }

    #[test]
    fn unique_users_builds_expected_query() {
        let q = unique_users_query(&UniqueUsersParams {
            period: "7d".into(),
            app: Some("tome".into()),
            event_name: Some("tome.search".into()),
        });
        let json = serde_json::to_value(&q).unwrap();
        assert_eq!(json["measures"], serde_json::json!(["unique_installs"]));
        assert_eq!(json["time_range"], serde_json::json!({"last": "7d"}));
        assert_eq!(json["filters"].as_array().unwrap().len(), 2);
        gauge_query::validate(&q).unwrap();
    }

    #[test]
    fn top_events_defaults_and_orders_desc() {
        let q = top_events_query(&TopEventsParams {
            period: "30d".into(),
            app: None,
            by: None,
            limit: None,
        });
        let json = serde_json::to_value(&q).unwrap();
        assert_eq!(json["dimensions"], serde_json::json!(["event_name"]));
        assert_eq!(
            json["order"],
            serde_json::json!([{"field": "count", "dir": "desc"}])
        );
        assert_eq!(json["limit"], 10);
        gauge_query::validate(&q).unwrap();
    }

    #[test]
    fn events_over_time_sets_granularity() {
        let q = events_over_time_query(&EventsOverTimeParams {
            period: "7d".into(),
            granularity: gauge_query::Granularity::Day,
            app: Some("midnight-manual".into()),
            event_name: None,
        });
        let json = serde_json::to_value(&q).unwrap();
        assert_eq!(json["granularity"], "day");
        gauge_query::validate(&q).unwrap();
    }

    #[test]
    fn numeric_histogram_builds_bucket_dimension() {
        let q = numeric_histogram_query(&NumericHistogramParams {
            period: "7d".into(),
            field: "latency_ms".into(),
            edges: vec![50.0, 200.0, 500.0],
            app: Some("tome".into()),
            event_name: None,
        })
        .unwrap();
        // Must have exactly one Bucket dimension with the supplied edges.
        assert_eq!(q.dimensions.len(), 1);
        match &q.dimensions[0] {
            gauge_query::Dimension::Bucket { bucket } => {
                assert_eq!(bucket.field.to_string(), "attr.latency_ms");
                assert_eq!(bucket.edges, vec![50.0, 200.0, 500.0]);
            }
            other => panic!("expected Bucket dimension, got {other:?}"),
        }
        // Measures must include Count.
        assert!(
            q.measures
                .iter()
                .any(|m| matches!(m, gauge_query::Measure::Count)),
            "expected Count in measures"
        );
        gauge_query::validate(&q).unwrap();
    }

    #[test]
    fn tool_param_schemas_generate_and_describe_fields() {
        // Guards the MCP tool surface: schemars must produce schemas agents can read.
        let schema = serde_json::to_value(schemars::schema_for!(UniqueUsersParams)).unwrap();
        assert!(schema["properties"]["period"].is_object());
        let schema =
            serde_json::to_value(schemars::schema_for!(gauge_query::QueryRequest)).unwrap();
        let props = schema["properties"].as_object().unwrap();
        for key in [
            "measures",
            "dimensions",
            "filters",
            "time_range",
            "granularity",
            "order",
            "limit",
        ] {
            assert!(
                props.contains_key(key),
                "QueryRequest schema missing `{key}`"
            );
        }
    }
}
