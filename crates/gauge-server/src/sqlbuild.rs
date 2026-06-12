//! Translates a validated QueryRequest into ONE parameterized SQL statement.
//! Identifiers come only from closed enums; every user-supplied value
//! (filter values, attr keys) is a bind parameter — never string-spliced.

use gauge_query::{
    DEFAULT_LIMIT, Dir, Field, FilterOp, FilterValue, MAX_LIMIT, Measure, QueryError, QueryRequest,
    resolve_time_range, validate,
};
use time::OffsetDateTime;

#[derive(Debug, Clone)]
pub enum Bind {
    Text(String),
    TextArr(Vec<String>),
    Time(OffsetDateTime),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColKind {
    Text,
    Int,
    TimeBucket,
}

#[derive(Debug, Clone)]
pub struct ColSpec {
    pub alias: String,
    pub kind: ColKind,
}

#[derive(Debug)]
pub struct BuiltQuery {
    pub sql: String,
    pub binds: Vec<Bind>,
    pub columns: Vec<ColSpec>,
    pub limit: usize,
}

fn ph(binds: &mut Vec<Bind>, b: Bind) -> String {
    binds.push(b);
    format!("${}", binds.len())
}

fn field_expr(f: &Field, binds: &mut Vec<Bind>) -> String {
    match f {
        Field::App => "app".into(),
        Field::EventName => "event_name".into(),
        Field::AppVersion => "app_version".into(),
        Field::Os => "os".into(),
        Field::Arch => "arch".into(),
        Field::Attr(k) => {
            let p = ph(binds, Bind::Text(k.clone()));
            format!("(attributes->>{p})")
        }
    }
}

pub fn build(req: &QueryRequest, now: OffsetDateTime) -> Result<BuiltQuery, QueryError> {
    validate(req)?;
    let (from, to) = resolve_time_range(&req.time_range, now)?;

    let mut binds: Vec<Bind> = Vec::new();
    let mut select: Vec<String> = Vec::new();
    let mut columns: Vec<ColSpec> = Vec::new();
    let mut group_count = 0usize;

    let p_from = ph(&mut binds, Bind::Time(from));
    let p_to = ph(&mut binds, Bind::Time(to));
    let mut wheres = vec![format!("time >= {p_from} AND time < {p_to}")];

    if let Some(g) = req.granularity {
        select.push(format!(
            "date_trunc('{}', time) AS \"time_bucket\"",
            g.date_trunc_unit()
        ));
        columns.push(ColSpec {
            alias: "time_bucket".into(),
            kind: ColKind::TimeBucket,
        });
        group_count += 1;
    }
    for d in &req.dimensions {
        // alias chars are restricted by Field::parse — safe inside quotes
        let alias = d.to_string();
        let expr = field_expr(d, &mut binds);
        select.push(format!("{expr} AS \"{alias}\""));
        columns.push(ColSpec {
            alias,
            kind: ColKind::Text,
        });
        group_count += 1;
    }
    for m in &req.measures {
        let expr = match m {
            Measure::Count => "COUNT(*)",
            Measure::UniqueInstalls => "COUNT(DISTINCT install_id)",
            Measure::UniqueSessions => "COUNT(DISTINCT session_id)",
        };
        select.push(format!("{expr} AS \"{}\"", m.alias()));
        columns.push(ColSpec {
            alias: m.alias().into(),
            kind: ColKind::Int,
        });
    }

    for f in &req.filters {
        let expr = field_expr(&f.field, &mut binds);
        match (f.op, f.value.as_ref()) {
            (FilterOp::Eq, Some(FilterValue::One(v))) => {
                let p = ph(&mut binds, Bind::Text(v.clone()));
                wheres.push(format!("{expr} = {p}"));
            }
            (FilterOp::Neq, Some(FilterValue::One(v))) => {
                let p = ph(&mut binds, Bind::Text(v.clone()));
                wheres.push(format!("{expr} <> {p}"));
            }
            (FilterOp::In, Some(FilterValue::Many(v))) => {
                let p = ph(&mut binds, Bind::TextArr(v.clone()));
                wheres.push(format!("{expr} = ANY({p})"));
            }
            (FilterOp::Exists, None) => {
                let Field::Attr(k) = &f.field else {
                    unreachable!("validated")
                };
                let p = ph(&mut binds, Bind::Text(k.clone()));
                wheres.push(format!("attributes ? {p}"));
            }
            _ => unreachable!("rejected by validate()"),
        }
    }

    let mut sql = format!(
        "SELECT {} FROM events WHERE {}",
        select.join(", "),
        wheres.join(" AND ")
    );
    if group_count > 0 {
        let ordinals: Vec<String> = (1..=group_count).map(|i| i.to_string()).collect();
        sql.push_str(&format!(" GROUP BY {}", ordinals.join(", ")));
    }

    let aliases: Vec<&str> = columns.iter().map(|c| c.alias.as_str()).collect();
    let order_terms: Vec<String> = if req.order.is_empty() {
        if req.granularity.is_some() {
            vec!["\"time_bucket\" ASC".into()]
        } else {
            vec![]
        }
    } else {
        req.order
            .iter()
            .map(|o| {
                if !aliases.contains(&o.field.as_str()) {
                    return Err(QueryError::BadOrderField(o.field.clone()));
                }
                let dir = match o.dir {
                    Dir::Asc => "ASC",
                    Dir::Desc => "DESC",
                };
                Ok(format!("\"{}\" {dir}", o.field))
            })
            .collect::<Result<_, _>>()?
    };
    if !order_terms.is_empty() {
        sql.push_str(&format!(" ORDER BY {}", order_terms.join(", ")));
    }

    let limit = req.limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT) as usize;
    sql.push_str(&format!(" LIMIT {}", limit + 1)); // +1 row to detect truncation
    Ok(BuiltQuery {
        sql,
        binds,
        columns,
        limit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gauge_query::*;
    use time::macros::datetime;

    const NOW: time::OffsetDateTime = datetime!(2026-06-12 12:00:00 UTC);

    fn spec_example() -> QueryRequest {
        serde_json::from_str(
            r#"{
              "measures": ["unique_installs"],
              "dimensions": ["app", "event_name"],
              "filters": [{"field": "app", "op": "eq", "value": "tome"},
                          {"field": "attr.surface", "op": "eq", "value": "mcp"}],
              "time_range": {"last": "7d"},
              "granularity": "day",
              "order": [{"field": "unique_installs", "dir": "desc"}],
              "limit": 100
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn snapshot_spec_example() {
        let built = build(&spec_example(), NOW).unwrap();
        insta::assert_snapshot!(built.sql);
        assert_eq!(built.limit, 100);
        assert_eq!(built.columns.len(), 4); // time_bucket, app, event_name, unique_installs
    }

    #[test]
    fn snapshot_minimal_count() {
        let req: QueryRequest =
            serde_json::from_str(r#"{"measures":["count"],"time_range":{"last":"24h"}}"#).unwrap();
        insta::assert_snapshot!(build(&req, NOW).unwrap().sql);
    }

    #[test]
    fn snapshot_in_and_exists_filters() {
        let req: QueryRequest = serde_json::from_str(
            r#"{"measures":["count"],"dimensions":["os"],
                "filters":[{"field":"event_name","op":"in","value":["tome.search","tome.install"]},
                           {"field":"attr.surface","op":"exists"}],
                "time_range":{"last":"30d"}}"#,
        )
        .unwrap();
        insta::assert_snapshot!(build(&req, NOW).unwrap().sql);
    }

    #[test]
    fn user_values_never_appear_in_sql_text() {
        let built = build(&spec_example(), NOW).unwrap();
        assert!(!built.sql.contains("tome"));
        assert!(!built.sql.contains("mcp"));
        // attr keys are bound too, not spliced
        assert!(!built.sql.contains("surface"));
    }

    #[test]
    fn order_must_reference_selected_alias() {
        let mut req = spec_example();
        req.order = vec![Order {
            field: "nope".into(),
            dir: Dir::Desc,
        }];
        assert!(matches!(build(&req, NOW), Err(QueryError::BadOrderField(f)) if f == "nope"));
    }

    #[test]
    fn default_limit_and_truncation_headroom() {
        let req: QueryRequest =
            serde_json::from_str(r#"{"measures":["count"],"time_range":{"last":"1d"}}"#).unwrap();
        let built = build(&req, NOW).unwrap();
        assert_eq!(built.limit, 1000);
        assert!(built.sql.ends_with("LIMIT 1001"));
    }
}
