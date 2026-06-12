//! Gauge OTLP profile validation: required resource attributes, event naming,
//! and hygiene limits. Shared by the server (ingest) and senders (pre-flight).

use serde_json::{Map, Value};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::otlp::{ExportLogsServiceRequest, KeyValue, LogRecord};

pub const MAX_ATTRIBUTES_PER_RECORD: usize = 30;
pub const MAX_ATTR_STRING_BYTES: usize = 128;
pub const MAX_RECORDS_PER_BATCH: usize = 1000;
pub const MAX_BODY_BYTES: usize = 1_048_576;
pub const OS_TYPES: &[&str] = &["darwin", "linux", "windows"];
pub const HOST_ARCHS: &[&str] = &["amd64", "arm64"];

#[derive(Debug, Clone, PartialEq)]
pub struct ResourceInfo {
    pub app: String,
    pub app_version: String,
    pub install_id: Uuid,
    pub session_id: Uuid,
    pub os: String,
    pub arch: String,
}

#[derive(Debug, Clone)]
pub struct ParsedEvent {
    pub event_name: String,
    pub time: OffsetDateTime,
    pub attributes: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rejection {
    pub index: usize,
    pub reason: String,
}

#[derive(Debug)]
pub struct ValidatedBatch {
    pub resource: ResourceInfo,
    pub events: Vec<ParsedEvent>,
    pub rejections: Vec<Rejection>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum BatchError {
    #[error("request must contain exactly one resourceLogs block")]
    ExpectedSingleResource,
    #[error("missing or invalid required resource attribute `{0}`")]
    BadResourceAttr(&'static str),
    #[error("service.name `{0}` is not in the app allowlist")]
    UnknownApp(String),
    #[error("batch exceeds {MAX_RECORDS_PER_BATCH} records")]
    TooManyRecords,
}

fn attr_str<'a>(attrs: &'a [KeyValue], key: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|kv| kv.key == key)
        .and_then(|kv| kv.value.string_value.as_deref())
}

pub fn validate_batch(
    req: &ExportLogsServiceRequest,
    allowlist: &[String],
) -> Result<ValidatedBatch, BatchError> {
    let [rl] = req.resource_logs.as_slice() else {
        return Err(BatchError::ExpectedSingleResource);
    };
    let res_attrs = rl
        .resource
        .as_ref()
        .map(|r| r.attributes.as_slice())
        .unwrap_or(&[]);

    let app = attr_str(res_attrs, "service.name")
        .filter(|s| !s.is_empty())
        .ok_or(BatchError::BadResourceAttr("service.name"))?
        .to_string();
    if !allowlist.iter().any(|a| a == &app) {
        return Err(BatchError::UnknownApp(app));
    }
    let app_version = attr_str(res_attrs, "service.version")
        .filter(|s| !s.is_empty())
        .ok_or(BatchError::BadResourceAttr("service.version"))?
        .to_string();
    let install_id = attr_str(res_attrs, "service.instance.id")
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or(BatchError::BadResourceAttr("service.instance.id"))?;
    let session_id = attr_str(res_attrs, "session.id")
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or(BatchError::BadResourceAttr("session.id"))?;
    let os = attr_str(res_attrs, "os.type")
        .filter(|s| OS_TYPES.contains(s))
        .ok_or(BatchError::BadResourceAttr("os.type"))?
        .to_string();
    let arch = attr_str(res_attrs, "host.arch")
        .filter(|s| HOST_ARCHS.contains(s))
        .ok_or(BatchError::BadResourceAttr("host.arch"))?
        .to_string();

    let records: Vec<&LogRecord> = rl.scope_logs.iter().flat_map(|s| &s.log_records).collect();
    if records.len() > MAX_RECORDS_PER_BATCH {
        return Err(BatchError::TooManyRecords);
    }

    let resource = ResourceInfo { app, app_version, install_id, session_id, os, arch };
    let mut events = Vec::new();
    let mut rejections = Vec::new();
    for (index, rec) in records.iter().enumerate() {
        match parse_record(rec, &resource.app) {
            Ok(ev) => events.push(ev),
            Err(reason) => rejections.push(Rejection { index, reason }),
        }
    }
    Ok(ValidatedBatch { resource, events, rejections })
}

fn parse_record(rec: &LogRecord, app: &str) -> Result<ParsedEvent, String> {
    let event_name = rec
        .event_name
        .clone()
        .or_else(|| {
            rec.attributes
                .iter()
                .find(|kv| kv.key == "event.name")
                .and_then(|kv| kv.value.string_value.clone())
        })
        .ok_or_else(|| "missing event name (eventName field or event.name attribute)".to_string())?;
    if !event_name.starts_with(&format!("{app}.")) {
        return Err(format!("event name must be prefixed with `{app}.`"));
    }

    let nanos = rec
        .time_unix_nano
        .filter(|n| *n > 0)
        .ok_or_else(|| "missing or zero timeUnixNano".to_string())?;
    let time = OffsetDateTime::from_unix_timestamp_nanos(nanos as i128)
        .map_err(|_| "timeUnixNano out of range".to_string())?;

    let attrs: Vec<&KeyValue> = rec.attributes.iter().filter(|kv| kv.key != "event.name").collect();
    if attrs.len() > MAX_ATTRIBUTES_PER_RECORD {
        return Err(format!("more than {MAX_ATTRIBUTES_PER_RECORD} attributes"));
    }

    let mut attributes = Map::new();
    for kv in attrs {
        let v = &kv.value;
        let value = if let Some(s) = &v.string_value {
            if s.len() > MAX_ATTR_STRING_BYTES {
                return Err(format!("attribute `{}` exceeds {MAX_ATTR_STRING_BYTES} bytes", kv.key));
            }
            Value::String(s.clone())
        } else if let Some(b) = v.bool_value {
            Value::Bool(b)
        } else if let Some(i) = &v.int_value {
            Value::Number(
                i.parse::<i64>()
                    .map_err(|_| format!("attribute `{}` has invalid intValue", kv.key))?
                    .into(),
            )
        } else if let Some(d) = v.double_value {
            serde_json::Number::from_f64(d)
                .map(Value::Number)
                .ok_or_else(|| format!("attribute `{}` has non-finite doubleValue", kv.key))?
        } else {
            return Err(format!("attribute `{}` has unsupported value type", kv.key));
        };
        attributes.insert(kv.key.clone(), value);
    }
    Ok(ParsedEvent { event_name, time, attributes })
}
