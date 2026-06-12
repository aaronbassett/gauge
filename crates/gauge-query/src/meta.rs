use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MetaResponse {
    pub apps: Vec<AppMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AppMeta {
    pub app: String,
    pub event_names: Vec<String>,
    pub attribute_keys: Vec<String>,
    /// RFC3339, None when the app has no events.
    pub first_event: Option<String>,
    pub last_event: Option<String>,
    pub total_events: i64,
}
