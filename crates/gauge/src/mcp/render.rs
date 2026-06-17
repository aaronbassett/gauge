//! Shapes every MCP tool result into the "summary + structuredContent" form.
//!
//! Success → one text block (summary + trimmed JSON fence) plus full-fidelity
//! `structured_content` (with `suggested_next_actions`). Failure → an
//! `is_error: true` result carrying a shared error envelope.

use rmcp::model::{CallToolResult, Content};
use serde_json::{Value, json};

/// A suggested follow-up. `tool: None` describes a user action (e.g. "run `gauge login`").
#[derive(Debug, Clone)]
pub struct NextAction {
    pub description: String,
    pub tool: Option<&'static str>,
    pub arguments: Option<Value>,
}

impl NextAction {
    pub fn call(description: impl Into<String>, tool: &'static str, arguments: Value) -> Self {
        Self {
            description: description.into(),
            tool: Some(tool),
            arguments: Some(arguments),
        }
    }
    pub fn user(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            tool: None,
            arguments: None,
        }
    }
    fn to_value(&self) -> Value {
        let mut o = json!({ "description": self.description });
        if let Some(t) = self.tool {
            o["tool"] = json!(t);
        }
        if let Some(a) = &self.arguments {
            o["arguments"] = a.clone();
        }
        o
    }
}

fn actions_value(actions: &[NextAction]) -> Value {
    Value::Array(actions.iter().map(NextAction::to_value).collect())
}

/// A successful tool result, pre-render.
pub struct ToolOutcome {
    pub summary: String,
    pub trimmed: Value,
    pub structured: Value,
    pub next_actions: Vec<NextAction>,
}

impl ToolOutcome {
    pub fn into_result(self) -> CallToolResult {
        let mut structured = self.structured;
        if let Value::Object(map) = &mut structured {
            map.insert(
                "suggested_next_actions".to_owned(),
                actions_value(&self.next_actions),
            );
        }
        let trimmed = serde_json::to_string(&self.trimmed).unwrap_or_else(|_| "{}".to_owned());
        let text = format!("{}\n\n```json\n{trimmed}\n```", self.summary);
        let mut result = CallToolResult::success(vec![Content::text(text)]);
        result.structured_content = Some(structured);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_renders_summary_fence_and_structured() {
        let outcome = ToolOutcome {
            summary: "2 rows, 9ms.".to_owned(),
            trimmed: json!({ "rows": 2 }),
            structured: json!({ "rows": [], "truncated": false, "elapsed_ms": 9 }),
            next_actions: vec![NextAction::call("Discover schema", "get_meta", json!({}))],
        };
        let result = outcome.into_result();
        assert_eq!(result.is_error, Some(false));
        // text block: summary then a ```json fence. `Content::as_text()` returns
        // `Option<&RawTextContent>`; `.text` is the string field.
        let text = result.content[0]
            .as_text()
            .expect("text content")
            .text
            .clone();
        assert!(text.starts_with("2 rows, 9ms."));
        assert!(text.contains("```json"));
        // structured_content carries the payload + injected suggested_next_actions
        let sc = result
            .structured_content
            .expect("structured_content present");
        assert_eq!(sc["truncated"], json!(false));
        assert_eq!(sc["suggested_next_actions"][0]["tool"], json!("get_meta"));
    }
}
