use crate::validate::QueryError;

/// Queryable fields: typed envelope columns or `attr.<key>` JSONB extractions.
/// install_id/session_id are deliberately NOT addressable (anonymity: no
/// per-install drill-down through the API).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Field {
    App,
    EventName,
    AppVersion,
    Os,
    Arch,
    Attr(String),
}

impl Field {
    pub fn parse(s: &str) -> Result<Self, QueryError> {
        Ok(match s {
            "app" => Self::App,
            "event_name" => Self::EventName,
            "app_version" => Self::AppVersion,
            "os" => Self::Os,
            "arch" => Self::Arch,
            other => {
                let key = other
                    .strip_prefix("attr.")
                    .filter(|k| {
                        !k.is_empty()
                            && k.len() <= 64
                            && k.chars()
                                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
                    })
                    .ok_or_else(|| QueryError::UnknownField(other.to_string()))?;
                Self::Attr(key.to_string())
            }
        })
    }
}

impl std::fmt::Display for Field {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::App => f.write_str("app"),
            Self::EventName => f.write_str("event_name"),
            Self::AppVersion => f.write_str("app_version"),
            Self::Os => f.write_str("os"),
            Self::Arch => f.write_str("arch"),
            Self::Attr(k) => write!(f, "attr.{k}"),
        }
    }
}

impl serde::Serialize for Field {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for Field {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

impl schemars::JsonSchema for Field {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Field".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "description": "One of: app, event_name, app_version, os, arch, or attr.<key> for an event attribute"
        })
    }
}
