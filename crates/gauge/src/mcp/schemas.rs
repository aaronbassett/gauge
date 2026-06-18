//! Hand-authored `outputSchema` per tool, injected onto router-built tools by
//! name in `ServerHandler::list_tools`. Unlike a passthrough schema, each one
//! describes the actual payload envelope.

use std::sync::Arc;

use rmcp::model::Tool;
use serde_json::{Value, json};

fn next_actions_fragment() -> Value {
    json!({
        "type": "array",
        "items": {
            "type": "object",
            "required": ["description"],
            "properties": {
                "description": { "type": "string", "description": "What this suggested action achieves." },
                "tool": { "type": "string", "description": "Tool to call. Absent for user actions." },
                "arguments": { "type": "object" }
            }
        }
    })
}

/// Shared envelope for the four query-returning tools.
fn query_envelope_schema() -> Value {
    json!({
        "type": "object",
        "required": ["rows", "truncated", "elapsed_ms"],
        "additionalProperties": true,
        "properties": {
            "rows": {
                "type": "array",
                "items": { "type": "object", "additionalProperties": true },
                "description": "One object per row, keyed by output aliases (measure names, dimension strings, \"time_bucket\")."
            },
            "truncated": { "type": "boolean", "description": "True if the row cap was hit and rows were dropped." },
            "elapsed_ms": { "type": "integer" },
            "suggested_next_actions": next_actions_fragment()
        }
    })
}

fn meta_schema() -> Value {
    json!({
        "type": "object",
        "required": ["apps"],
        "additionalProperties": true,
        "properties": {
            "apps": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["app", "event_names", "attribute_keys", "total_events"],
                    "properties": {
                        "app": { "type": "string" },
                        "event_names": { "type": "array", "items": { "type": "string" } },
                        "attribute_keys": { "type": "array", "items": { "type": "string" } },
                        "numeric_attribute_keys": { "type": "array", "items": { "type": "string" } },
                        "first_event": { "type": "string", "description": "RFC3339; absent when the app has no events." },
                        "last_event": { "type": "string", "description": "RFC3339; absent when the app has no events." },
                        "total_events": { "type": "integer" }
                    }
                }
            },
            "suggested_next_actions": next_actions_fragment()
        }
    })
}

fn schema_for(name: &str) -> Option<Value> {
    match name {
        "get_meta" => Some(meta_schema()),
        "query_telemetry" | "unique_users" | "top_events" | "events_over_time"
        | "numeric_stats" | "numeric_histogram" => Some(query_envelope_schema()),
        _ => None,
    }
}

/// Patch hand-authored output schemas onto router-built tools, matched by name.
pub fn apply_output_schemas(tools: &mut [Tool]) {
    for t in tools.iter_mut() {
        if let Some(Value::Object(map)) = schema_for(&t.name) {
            t.output_schema = Some(Arc::new(map));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::Tool;
    use std::sync::Arc;

    #[test]
    fn query_envelope_describes_real_fields() {
        let s = query_envelope_schema();
        let props = s["properties"].as_object().unwrap();
        for key in ["rows", "truncated", "elapsed_ms", "suggested_next_actions"] {
            assert!(props.contains_key(key), "query envelope missing `{key}`");
        }
        assert_eq!(s["required"], json!(["rows", "truncated", "elapsed_ms"]));
    }

    #[test]
    fn meta_schema_lists_app_fields() {
        let s = meta_schema();
        let app_props = s["properties"]["apps"]["items"]["properties"]
            .as_object()
            .unwrap();
        for key in ["app", "event_names", "attribute_keys", "total_events"] {
            assert!(
                app_props.contains_key(key),
                "meta app schema missing `{key}`"
            );
        }
        assert!(
            app_props.contains_key("numeric_attribute_keys"),
            "meta app schema missing `numeric_attribute_keys`"
        );
    }

    #[test]
    fn apply_sets_schema_for_known_tools_only() {
        let empty = Arc::new(serde_json::Map::new());
        let mut tools = vec![
            Tool::new("query_telemetry", "q", empty.clone()),
            Tool::new("get_meta", "m", empty.clone()),
            Tool::new("some_unknown_tool", "u", empty.clone()),
        ];
        apply_output_schemas(&mut tools);
        assert!(tools[0].output_schema.is_some());
        assert!(tools[1].output_schema.is_some());
        assert!(tools[2].output_schema.is_none());
    }
}
