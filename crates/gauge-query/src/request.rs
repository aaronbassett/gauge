use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::field::Field;

pub const DEFAULT_LIMIT: u32 = 1_000;
pub const MAX_LIMIT: u32 = 10_000;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QueryRequest {
    pub measures: Vec<Measure>,
    #[serde(default)]
    pub dimensions: Vec<Field>,
    #[serde(default)]
    pub filters: Vec<Filter>,
    pub time_range: TimeRange,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub granularity: Option<Granularity>,
    #[serde(default)]
    pub order: Vec<Order>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Measure {
    Count,
    UniqueInstalls,
    UniqueSessions,
}

impl Measure {
    pub fn alias(&self) -> &'static str {
        match self {
            Self::Count => "count",
            Self::UniqueInstalls => "unique_installs",
            Self::UniqueSessions => "unique_sessions",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Filter {
    pub field: Field,
    pub op: FilterOp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<FilterValue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FilterOp {
    Eq,
    Neq,
    In,
    Exists,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum FilterValue {
    One(String),
    Many(Vec<String>),
}

/// Relative ranges use "<N>h" or "<N>d" (max 365d). Absolute uses RFC3339.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum TimeRange {
    Last { last: String },
    Absolute { from: String, to: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Granularity {
    Hour,
    Day,
    Week,
}

impl Granularity {
    pub fn date_trunc_unit(&self) -> &'static str {
        match self {
            Self::Hour => "hour",
            Self::Day => "day",
            Self::Week => "week",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Order {
    /// References an output alias: a measure name, a dimension string, or "time_bucket".
    pub field: String,
    pub dir: Dir,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Dir {
    Asc,
    Desc,
}
