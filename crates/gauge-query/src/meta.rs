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
    /// Subset of `attribute_keys` whose values are JSON numbers (bucketable/aggregatable).
    #[serde(default)]
    pub numeric_attribute_keys: Vec<String>,
    /// RFC3339, None when the app has no events.
    pub first_event: Option<String>,
    pub last_event: Option<String>,
    pub total_events: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn appmeta_has_numeric_attribute_keys() {
        let schema = serde_json::to_value(schemars::schema_for!(AppMeta)).unwrap();
        assert!(schema["properties"]["numeric_attribute_keys"].is_object());
    }
}
