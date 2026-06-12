use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueryResponse {
    /// One JSON object per row, keyed by output aliases
    /// (measure names, dimension strings, "time_bucket").
    pub rows: Vec<serde_json::Value>,
    pub truncated: bool,
    pub elapsed_ms: u64,
}
