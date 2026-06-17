//! Pure parameter→QueryRequest builders for the MCP convenience tools.
//! Separated from rmcp glue so they unit-test without a server.

use gauge_query::{
    Dir, Field, Filter, FilterOp, FilterValue, Granularity, Measure, Order, QueryRequest, TimeRange,
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
        dimensions: vec![Field::EventName],
        filters: base_filters(&p.app, &None),
        time_range: TimeRange::Last { last: p.period.clone() },
        granularity: None,
        order: vec![Order { field: order_field, dir: Dir::Desc }],
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

#[cfg(test)]
mod tests {
    use super::*;

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
