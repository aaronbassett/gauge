//! The `Event` trait and the serialize → flat scalar-attribute-map conversion
//! that every emitted event passes through. Non-scalar fields are rejected;
//! this is the structural half of the privacy contract (canary tests are the
//! other half — see `canary`).

use std::borrow::Cow;

use serde::Serialize;
use serde_json::{Map, Value};

use gauge_events::profile::{MAX_ATTR_STRING_BYTES, MAX_ATTRIBUTES_PER_RECORD};

/// Anything an app emits. The bare `name()` is namespaced with `<app>.` by the
/// client; the value must serialize (via serde) to a flat JSON object whose
/// values are all scalars (string / bool / number).
///
/// `Option` fields MUST use `#[serde(skip_serializing_if = "Option::is_none")]`
/// so a genuinely-absent field is omitted rather than encoded as `null` (a
/// `null` is rejected by [`to_attributes`]). Integer fields must fit `i64`/`u64`;
/// a wider value fails serialization and the whole event is dropped.
pub trait Event: Serialize {
    /// Bare event name, no app prefix, e.g. `"search"` → `"tome.search"`.
    fn name(&self) -> Cow<'_, str>;
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum EmitError {
    #[error("event must serialize to a JSON object")]
    NotAnObject,
    #[error("event could not be serialized: {0}")]
    Unserializable(String),
    #[error("attribute `{0}` is not a scalar (string/bool/number)")]
    NonScalar(String),
    #[error("attribute `{0}` string value exceeds {MAX_ATTR_STRING_BYTES} bytes")]
    StringTooLong(String),
    #[error("event has {0} attributes, exceeding {MAX_ATTRIBUTES_PER_RECORD}")]
    TooManyAttributes(usize),
}

/// Convert an event into the flat scalar attribute map the sender expects.
///
/// Every field must be a scalar (string / bool / number) or omitted via
/// `#[serde(skip_serializing_if = "Option::is_none")]`. A `null` value is
/// rejected as [`EmitError::NonScalar`] — this includes a non-finite float
/// (`NaN`/`Inf`), which serde encodes as `null`, since the server rejects
/// non-finite doubles. Integer fields must fit `i64`/`u64`; a wider value makes
/// serialization fail and is reported as [`EmitError::Unserializable`].
pub fn to_attributes<E: Event + ?Sized>(event: &E) -> Result<Map<String, Value>, EmitError> {
    let value =
        serde_json::to_value(event).map_err(|e| EmitError::Unserializable(e.to_string()))?;
    let Value::Object(obj) = value else {
        return Err(EmitError::NotAnObject);
    };
    let mut out = Map::new();
    for (k, v) in obj {
        match v {
            Value::Null => return Err(EmitError::NonScalar(k)),
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

/// Bare event names must be a single non-empty segment (the client prefixes
/// `<app>.`). Reject empty, names containing `.`, leading/trailing whitespace,
/// or names longer than 128 bytes.
pub fn is_valid_event_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 128 && !name.contains('.') && name.trim() == name
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
        let e = Sample {
            s: "x".into(),
            n: 7,
            b: true,
            maybe: None,
        };
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
        let e = LongString {
            big: "x".repeat(MAX_ATTR_STRING_BYTES + 1),
        };
        assert_eq!(
            to_attributes(&e),
            Err(EmitError::StringTooLong("big".into()))
        );
    }

    #[derive(Serialize)]
    struct NonFinite {
        f: f64,
    }
    impl Event for NonFinite {
        fn name(&self) -> Cow<'_, str> {
            "nonfinite".into()
        }
    }

    #[test]
    fn non_finite_float_is_rejected_not_dropped() {
        // serde encodes NaN/Inf as JSON null; the server rejects non-finite
        // doubles, so we must reject rather than silently drop the attribute.
        let e = NonFinite { f: f64::NAN };
        assert_eq!(to_attributes(&e), Err(EmitError::NonScalar("f".into())));
    }

    #[derive(Serialize)]
    struct OutOfRange {
        x: u128,
    }
    impl Event for OutOfRange {
        fn name(&self) -> Cow<'_, str> {
            "outofrange".into()
        }
    }

    #[test]
    fn out_of_range_integer_is_unserializable_not_not_an_object() {
        // `u64::MAX + 1` overflows serde_json's number range; this must surface
        // as `Unserializable`, not the misleading `NotAnObject`.
        let e = OutOfRange {
            x: u128::from(u64::MAX) + 1,
        };
        match to_attributes(&e) {
            Err(EmitError::Unserializable(_)) => {}
            other => panic!("expected Unserializable, got {other:?}"),
        }
    }

    #[test]
    fn event_name_validation() {
        assert!(is_valid_event_name("search"));
        assert!(!is_valid_event_name(""));
        assert!(!is_valid_event_name("a.b"));
        assert!(!is_valid_event_name(" x"));
        assert!(!is_valid_event_name(&"x".repeat(129)));
    }
}
