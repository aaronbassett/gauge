use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::field::Field;

pub const DEFAULT_LIMIT: u32 = 1_000;
pub const MAX_LIMIT: u32 = 10_000;
pub const MAX_BUCKET_EDGES: usize = 32;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QueryRequest {
    pub measures: Vec<Measure>,
    #[serde(default)]
    pub dimensions: Vec<Dimension>,
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

#[derive(Debug, Clone, PartialEq)]
pub enum Measure {
    Count,
    UniqueInstalls,
    UniqueSessions,
    Avg(Field),
    Min(Field),
    Max(Field),
    P50(Field),
    P90(Field),
    P95(Field),
    P99(Field),
}

impl Measure {
    /// Output column alias. Aggregates → `{fn}_{attr-key}` (e.g. `avg_latency_ms`).
    pub fn alias(&self) -> String {
        fn key(f: &Field) -> String {
            match f {
                Field::Attr(k) => k.clone(),
                other => other.to_string(),
            }
        }
        match self {
            Measure::Count => "count".into(),
            Measure::UniqueInstalls => "unique_installs".into(),
            Measure::UniqueSessions => "unique_sessions".into(),
            Measure::Avg(f) => format!("avg_{}", key(f)),
            Measure::Min(f) => format!("min_{}", key(f)),
            Measure::Max(f) => format!("max_{}", key(f)),
            Measure::P50(f) => format!("p50_{}", key(f)),
            Measure::P90(f) => format!("p90_{}", key(f)),
            Measure::P95(f) => format!("p95_{}", key(f)),
            Measure::P99(f) => format!("p99_{}", key(f)),
        }
    }
    /// The numeric attr field an aggregate operates on, if any.
    pub fn numeric_field(&self) -> Option<&Field> {
        match self {
            Measure::Avg(f)
            | Measure::Min(f)
            | Measure::Max(f)
            | Measure::P50(f)
            | Measure::P90(f)
            | Measure::P95(f)
            | Measure::P99(f) => Some(f),
            _ => None,
        }
    }
}

impl serde::Serialize for Measure {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let (name, field) = match self {
            Measure::Count => return s.serialize_str("count"),
            Measure::UniqueInstalls => return s.serialize_str("unique_installs"),
            Measure::UniqueSessions => return s.serialize_str("unique_sessions"),
            Measure::Avg(f) => ("avg", f),
            Measure::Min(f) => ("min", f),
            Measure::Max(f) => ("max", f),
            Measure::P50(f) => ("p50", f),
            Measure::P90(f) => ("p90", f),
            Measure::P95(f) => ("p95", f),
            Measure::P99(f) => ("p99", f),
        };
        let mut m = s.serialize_map(Some(1))?;
        m.serialize_entry(name, field)?;
        m.end()
    }
}

impl<'de> serde::Deserialize<'de> for Measure {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = Measure;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a measure name or a single-key aggregate object")
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Measure, E> {
                match v {
                    "count" => Ok(Measure::Count),
                    "unique_installs" => Ok(Measure::UniqueInstalls),
                    "unique_sessions" => Ok(Measure::UniqueSessions),
                    other => Err(E::custom(format!("unknown measure `{other}`"))),
                }
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Measure, A::Error> {
                let entry: Option<(String, Field)> = map.next_entry()?;
                let (name, field) = entry
                    .ok_or_else(|| serde::de::Error::custom("empty aggregate measure object"))?;
                if map.next_key::<String>()?.is_some() {
                    return Err(serde::de::Error::custom(
                        "aggregate measure must have exactly one key",
                    ));
                }
                match name.as_str() {
                    "avg" => Ok(Measure::Avg(field)),
                    "min" => Ok(Measure::Min(field)),
                    "max" => Ok(Measure::Max(field)),
                    "p50" => Ok(Measure::P50(field)),
                    "p90" => Ok(Measure::P90(field)),
                    "p95" => Ok(Measure::P95(field)),
                    "p99" => Ok(Measure::P99(field)),
                    other => Err(serde::de::Error::custom(format!(
                        "unknown aggregate `{other}`"
                    ))),
                }
            }
        }
        d.deserialize_any(V)
    }
}

impl schemars::JsonSchema for Measure {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Measure".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "A simple measure name, or a single-key aggregate object over a numeric attr.<key>.",
            "oneOf": [
                { "type": "string", "enum": ["count", "unique_installs", "unique_sessions"] },
                { "type": "object", "minProperties": 1, "maxProperties": 1, "additionalProperties": false,
                  "properties": {
                    "avg": {"type":"string"}, "min": {"type":"string"}, "max": {"type":"string"},
                    "p50": {"type":"string"}, "p90": {"type":"string"}, "p95": {"type":"string"}, "p99": {"type":"string"}
                  } }
            ]
        })
    }
}

// `Serialize`/`JsonSchema` stay derived (untagged wire form: a bare string for a
// field, a `{"bucket": ...}` object for a bucket). `Deserialize` is hand-written
// below so an unknown field name produces `Field::parse`'s helpful "unknown field
// `x`" error instead of serde's opaque "did not match any variant of untagged enum".
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Dimension {
    Field(Field),                  // "app", "attr.surface"
    Bucket { bucket: BucketSpec }, // {"bucket":{"field":"attr.latency_ms","edges":[50,200,500,1000]}}
}

impl<'de> serde::Deserialize<'de> for Dimension {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = Dimension;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a field name string or a {\"bucket\": {field, edges}} object")
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Dimension, E> {
                // Field::parse names the offending field on failure.
                Field::parse(v).map(Dimension::Field).map_err(E::custom)
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Dimension, A::Error> {
                let key: Option<String> = map.next_key()?;
                match key.as_deref() {
                    Some("bucket") => {
                        let bucket: BucketSpec = map.next_value()?;
                        if let Some(extra) = map.next_key::<String>()? {
                            return Err(serde::de::Error::custom(format!(
                                "unexpected key `{extra}` in bucket dimension"
                            )));
                        }
                        Ok(Dimension::Bucket { bucket })
                    }
                    Some(other) => Err(serde::de::Error::custom(format!(
                        "unknown dimension key `{other}` (expected a field name or `bucket`)"
                    ))),
                    None => Err(serde::de::Error::custom("empty dimension object")),
                }
            }
        }
        d.deserialize_any(V)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BucketSpec {
    pub field: Field,
    pub edges: Vec<f64>,
}

impl Dimension {
    pub fn field(&self) -> &Field {
        match self {
            Dimension::Field(f) => f,
            Dimension::Bucket { bucket } => &bucket.field,
        }
    }
    /// Output column alias (the field's display string).
    pub fn alias(&self) -> String {
        self.field().to_string()
    }
}

impl From<Field> for Dimension {
    fn from(f: Field) -> Self {
        Dimension::Field(f)
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
    Gt,
    Gte,
    Lt,
    Lte,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum FilterValue {
    One(String),
    Many(Vec<String>),
    Num(f64),
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

#[cfg(test)]
mod tests {
    use super::Measure;

    #[test]
    fn dimension_serde_field_and_bucket() {
        use crate::request::{Dimension, QueryRequest};
        // a bare string is a Field dimension (backward compatible)
        let f: Dimension = serde_json::from_value(serde_json::json!("app")).unwrap();
        assert!(matches!(f, Dimension::Field(_)));
        // an object is a bucket dimension
        let b: Dimension = serde_json::from_value(
            serde_json::json!({"bucket": {"field": "attr.latency_ms", "edges": [50, 200]}}),
        )
        .unwrap();
        assert!(matches!(b, Dimension::Bucket { .. }));
        // existing requests with string dimensions still parse
        let _: QueryRequest = serde_json::from_str(
            r#"{"measures":["count"],"dimensions":["app","event_name"],"time_range":{"last":"1d"}}"#,
        )
        .unwrap();
    }

    #[test]
    fn dimension_unknown_field_error_names_it() {
        use crate::request::{Dimension, QueryRequest};
        // A non-addressable / unknown dimension must surface a message that names the
        // offending field (the untagged-enum default would drop it). Guards the
        // DB-backed `invalid_query_is_422_naming_the_field` integration test without a DB.
        let err = serde_json::from_value::<Dimension>(serde_json::json!("install_id")).unwrap_err();
        assert!(
            err.to_string().contains("install_id"),
            "dimension error should name the field, got: {err}"
        );
        // ...and the same holds when nested in a full request.
        let err = serde_json::from_str::<QueryRequest>(
            r#"{"measures":["count"],"dimensions":["install_id"],"time_range":{"last":"1d"}}"#,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("install_id"),
            "request error should name the field, got: {err}"
        );
    }

    #[test]
    fn measure_serde_simple_and_aggregate() {
        // simple measures stay strings
        assert_eq!(
            serde_json::to_value(&Measure::Count).unwrap(),
            serde_json::json!("count")
        );
        let m: Measure = serde_json::from_value(serde_json::json!("unique_installs")).unwrap();
        assert_eq!(m, Measure::UniqueInstalls);
        // aggregates are single-key objects keyed by the agg name
        let avg: Measure =
            serde_json::from_value(serde_json::json!({"avg": "attr.latency_ms"})).unwrap();
        assert!(matches!(&avg, Measure::Avg(f) if f.to_string() == "attr.latency_ms"));
        assert_eq!(
            serde_json::to_value(&avg).unwrap(),
            serde_json::json!({"avg": "attr.latency_ms"})
        );
        let p95: Measure =
            serde_json::from_value(serde_json::json!({"p95": "attr.latency_ms"})).unwrap();
        assert_eq!(p95.alias(), "p95_latency_ms");
        // a two-key aggregate object is rejected
        assert!(
            serde_json::from_value::<Measure>(serde_json::json!({"avg":"attr.a","min":"attr.b"}))
                .is_err()
        );
    }
}
