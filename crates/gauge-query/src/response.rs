use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueryResponse {
    /// One JSON object per row, keyed by output aliases
    /// (measure names, dimension strings, "time_bucket").
    pub rows: Vec<serde_json::Value>,
    pub truncated: bool,
    pub elapsed_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<QueryMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueryMeta {
    pub buckets: Vec<BucketMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BucketMeta {
    pub field: String,
    pub alias: String,
    pub edges: Vec<f64>,
    pub labels: Vec<String>,
}
