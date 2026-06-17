//! The `Event` trait and the serialize → flat scalar-attribute-map conversion
//! that every emitted event passes through. Non-scalar fields are rejected;
//! this is the structural half of the privacy contract (canary tests are the
//! other half — see `canary`).

use std::borrow::Cow;

use serde::Serialize;
use serde_json::{Map, Value};

use gauge_events::profile::{MAX_ATTRIBUTES_PER_RECORD, MAX_ATTR_STRING_BYTES};

/// Anything an app emits. The bare `name()` is namespaced with `<app>.` by the
/// client; the value must serialize (via serde) to a flat JSON object whose
/// values are all scalars (string / bool / number).
pub trait Event: Serialize {
    /// Bare event name, no app prefix, e.g. `"search"` → `"tome.search"`.
    fn name(&self) -> Cow<'_, str>;
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum EmitError {
    #[error("event must serialize to a JSON object")]
    NotAnObject,
    #[error("attribute `{0}` is not a scalar (string/bool/number)")]
    NonScalar(String),
    #[error("attribute `{0}` string value exceeds {MAX_ATTR_STRING_BYTES} bytes")]
    StringTooLong(String),
    #[error("event has {0} attributes, exceeding {MAX_ATTRIBUTES_PER_RECORD}")]
    TooManyAttributes(usize),
}

/// Convert an event into the flat scalar attribute map the sender expects.
/// `null` values (e.g. `None` fields) are omitted; nested values are rejected.
pub fn to_attributes<E: Event + ?Sized>(event: &E) -> Result<Map<String, Value>, EmitError> {
    let value = serde_json::to_value(event).map_err(|_| EmitError::NotAnObject)?;
    let Value::Object(obj) = value else {
        return Err(EmitError::NotAnObject);
    };
    let mut out = Map::new();
    for (k, v) in obj {
        match v {
            Value::Null => continue,
            Value::String(s) => {
                if s.len() > MAX_ATTR_STRING_BYTES {
                    return Err(EmitError::StringTooLong(k));
                }
                out.insert(k, Value::String(s));
            }
            Value::Bool(_) | Value::Number(_) => {
                out.insert(k, v);
            }
            Value::Array(_) | Value::Object(_) => return Err(EmitError::NonScalar(k)),
        }
    }
    if out.len() > MAX_ATTRIBUTES_PER_RECORD {
        return Err(EmitError::TooManyAttributes(out.len()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize)]
    struct Sample {
        s: String,
        n: u32,
        b: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        maybe: Option<String>,
    }
    impl Event for Sample {
        fn name(&self) -> Cow<'_, str> {
            "sample".into()
        }
    }

    #[test]
    fn scalars_pass_and_none_is_omitted() {
        let e = Sample { s: "x".into(), n: 7, b: true, maybe: None };
        let attrs = to_attributes(&e).unwrap();
        assert_eq!(attrs.len(), 3);
        assert_eq!(attrs["s"], Value::String("x".into()));
        assert_eq!(attrs["n"], Value::Number(7u32.into()));
        assert_eq!(attrs["b"], Value::Bool(true));
        assert!(!attrs.contains_key("maybe"));
    }

    #[derive(Serialize)]
    struct Nested {
        inner: Vec<u8>,
    }
    impl Event for Nested {
        fn name(&self) -> Cow<'_, str> {
            "nested".into()
        }
    }

    #[test]
    fn nested_value_is_rejected() {
        let e = Nested { inner: vec![1, 2] };
        assert_eq!(to_attributes(&e), Err(EmitError::NonScalar("inner".into())));
    }

    #[derive(Serialize)]
    struct LongString {
        big: String,
    }
    impl Event for LongString {
        fn name(&self) -> Cow<'_, str> {
            "long".into()
        }
    }

    #[test]
    fn overlong_string_is_rejected() {
        let e = LongString { big: "x".repeat(MAX_ATTR_STRING_BYTES + 1) };
        assert_eq!(to_attributes(&e), Err(EmitError::StringTooLong("big".into())));
    }
}
