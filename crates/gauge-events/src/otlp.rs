//! Serde types for the subset of the OTLP/HTTP logs-signal JSON encoding that
//! the Gauge profile uses. Field names follow the protobuf JSON mapping
//! (camelCase; 64-bit integers encoded as decimal strings).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportLogsServiceRequest {
    #[serde(default)]
    pub resource_logs: Vec<ResourceLogs>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceLogs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<Resource>,
    #[serde(default)]
    pub scope_logs: Vec<ScopeLogs>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Resource {
    #[serde(default)]
    pub attributes: Vec<KeyValue>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeLogs {
    #[serde(default)]
    pub log_records: Vec<LogRecord>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogRecord {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "u64_string_opt"
    )]
    pub time_unix_nano: Option<u64>,
    /// OTLP >= 1.4 LogRecord event name field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_name: Option<String>,
    #[serde(default)]
    pub attributes: Vec<KeyValue>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KeyValue {
    pub key: String,
    #[serde(default)]
    pub value: AnyValue,
}

/// Protobuf JSON encodes AnyValue as a single-variant object,
/// e.g. {"stringValue": "x"} or {"intValue": "42"}.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnyValue {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub string_value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bool_value: Option<bool>,
    /// int64 as decimal string per the protobuf JSON mapping.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub int_value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub double_value: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportLogsServiceResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partial_success: Option<ExportLogsPartialSuccess>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportLogsPartialSuccess {
    pub rejected_log_records: i64,
    pub error_message: String,
}

/// u64 that serializes as a string but tolerantly deserializes from string or number.
mod u64_string_opt {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &Option<u64>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(n) => s.serialize_str(&n.to_string()),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            N(u64),
            S(String),
        }
        Ok(match Option::<Raw>::deserialize(d)? {
            None => None,
            Some(Raw::N(n)) => Some(n),
            Some(Raw::S(s)) => Some(s.parse().map_err(serde::de::Error::custom)?),
        })
    }
}
