//! Translates a validated QueryRequest into ONE parameterized SQL statement.
//! Identifiers come only from closed enums; every user-supplied value
//! (filter values, attr keys) is a bind parameter — never string-spliced.

use gauge_query::{
    BucketMeta, DEFAULT_LIMIT, Dimension, Dir, Field, FilterOp, FilterValue, MAX_LIMIT, Measure,
    QueryError, QueryRequest, bucket_labels, resolve_time_range, validate,
};
use serde_json::Value;
use time::OffsetDateTime;

#[derive(Debug, Clone)]
pub enum Bind {
    Text(String),
    TextArr(Vec<String>),
    Time(OffsetDateTime),
    Float(f64),
    FloatArr(Vec<f64>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ColKind {
    Text,
    Int,
    TimeBucket,
    Float,
    Bucket { labels: Vec<String> },
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
    pub bucket_meta: Vec<BucketMeta>,
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

/// Graceful JSONB→f64 cast: non-numeric / absent values become NULL (excluded
/// from aggregates/filters; never an error). The key is bound once and the
/// placeholder is referenced twice.
fn numeric_field_expr(f: &Field, binds: &mut Vec<Bind>) -> String {
    let Field::Attr(k) = f else {
        unreachable!("validated: numeric ops require an attr.<key> field")
    };
    let p = ph(binds, Bind::Text(k.clone()));
    format!(
        "CASE WHEN jsonb_typeof(attributes->{p}) = 'number' THEN (attributes->>{p})::double precision END"
    )
}

fn percentile_expr(f: &Field, frac: &str, binds: &mut Vec<Bind>) -> String {
    // `frac` is a fixed literal per measure variant — never user input.
    format!(
        "percentile_cont({frac}) WITHIN GROUP (ORDER BY {})",
        numeric_field_expr(f, binds)
    )
}

/// Decode an `f64` column (NULL → JSON null; non-finite → null).
pub fn float_value(v: Option<f64>) -> Value {
    v.and_then(serde_json::Number::from_f64)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

/// Decode a `width_bucket` index column to its human label.
pub fn bucket_value(labels: &[String], idx: Option<i32>) -> Value {
    match idx {
        Some(i) if i >= 0 && (i as usize) < labels.len() => {
            Value::String(labels[i as usize].clone())
        }
        _ => Value::Null,
    }
}

pub fn build(req: &QueryRequest, now: OffsetDateTime) -> Result<BuiltQuery, QueryError> {
    validate(req)?;
    let (from, to) = resolve_time_range(&req.time_range, now)?;

    let mut binds: Vec<Bind> = Vec::new();
    let mut select: Vec<String> = Vec::new();
    let mut columns: Vec<ColSpec> = Vec::new();
    let mut group_count = 0usize;
    let mut bucket_meta: Vec<BucketMeta> = Vec::new();

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
        match d {
            Dimension::Field(f) => {
                let alias = f.to_string();
                let expr = field_expr(f, &mut binds);
                select.push(format!("{expr} AS \"{alias}\""));
                columns.push(ColSpec {
                    alias,
                    kind: ColKind::Text,
                });
                group_count += 1;
            }
            Dimension::Bucket { bucket } => {
                let alias = bucket.field.to_string();
                let inner = numeric_field_expr(&bucket.field, &mut binds); // key bound once
                wheres.push(format!("{inner} IS NOT NULL")); // exclude non-numeric rows
                let pe = ph(&mut binds, Bind::FloatArr(bucket.edges.clone()));
                select.push(format!("width_bucket({inner}, {pe}) AS \"{alias}\""));
                let labels = bucket_labels(&bucket.edges);
                columns.push(ColSpec {
                    alias: alias.clone(),
                    kind: ColKind::Bucket {
                        labels: labels.clone(),
                    },
                });
                bucket_meta.push(BucketMeta {
                    field: alias.clone(),
                    alias,
                    edges: bucket.edges.clone(),
                    labels,
                });
                group_count += 1;
            }
        }
    }
    for m in &req.measures {
        let (expr, kind) = match m {
            Measure::Count => ("COUNT(*)".to_string(), ColKind::Int),
            Measure::UniqueInstalls => ("COUNT(DISTINCT install_id)".to_string(), ColKind::Int),
            Measure::UniqueSessions => ("COUNT(DISTINCT session_id)".to_string(), ColKind::Int),
            Measure::Avg(f) => (
                format!("AVG({})", numeric_field_expr(f, &mut binds)),
                ColKind::Float,
            ),
            Measure::Min(f) => (
                format!("MIN({})", numeric_field_expr(f, &mut binds)),
                ColKind::Float,
            ),
            Measure::Max(f) => (
                format!("MAX({})", numeric_field_expr(f, &mut binds)),
                ColKind::Float,
            ),
            Measure::P50(f) => (percentile_expr(f, "0.5", &mut binds), ColKind::Float),
            Measure::P90(f) => (percentile_expr(f, "0.9", &mut binds), ColKind::Float),
            Measure::P95(f) => (percentile_expr(f, "0.95", &mut binds), ColKind::Float),
            Measure::P99(f) => (percentile_expr(f, "0.99", &mut binds), ColKind::Float),
        };
        let alias = m.alias();
        select.push(format!("{expr} AS \"{alias}\""));
        columns.push(ColSpec { alias, kind });
    }

    for f in &req.filters {
        // `field_expr` is built INSIDE each value-comparison arm so it is only
        // bound when actually used. The `exists` arm builds its own expression
        // (`attributes ? key`) and must NOT call `field_expr`, or it would push
        // an unreferenced bind and leave a gap in the $N numbering.
        match (f.op, f.value.as_ref()) {
            (FilterOp::Eq, Some(FilterValue::One(v))) => {
                let expr = field_expr(&f.field, &mut binds);
                let p = ph(&mut binds, Bind::Text(v.clone()));
                wheres.push(format!("{expr} = {p}"));
            }
            (FilterOp::Neq, Some(FilterValue::One(v))) => {
                let expr = field_expr(&f.field, &mut binds);
                let p = ph(&mut binds, Bind::Text(v.clone()));
                wheres.push(format!("{expr} <> {p}"));
            }
            (FilterOp::In, Some(FilterValue::Many(v))) => {
                let expr = field_expr(&f.field, &mut binds);
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
            (FilterOp::Gt, Some(FilterValue::Num(v))) => {
                let expr = numeric_field_expr(&f.field, &mut binds);
                let p = ph(&mut binds, Bind::Float(*v));
                wheres.push(format!("{expr} > {p}"));
            }
            (FilterOp::Gte, Some(FilterValue::Num(v))) => {
                let expr = numeric_field_expr(&f.field, &mut binds);
                let p = ph(&mut binds, Bind::Float(*v));
                wheres.push(format!("{expr} >= {p}"));
            }
            (FilterOp::Lt, Some(FilterValue::Num(v))) => {
                let expr = numeric_field_expr(&f.field, &mut binds);
                let p = ph(&mut binds, Bind::Float(*v));
                wheres.push(format!("{expr} < {p}"));
            }
            (FilterOp::Lte, Some(FilterValue::Num(v))) => {
                let expr = numeric_field_expr(&f.field, &mut binds);
                let p = ph(&mut binds, Bind::Float(*v));
                wheres.push(format!("{expr} <= {p}"));
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
        let mut terms = Vec::new();
        if req.granularity.is_some() {
            terms.push("\"time_bucket\" ASC".to_string());
        }
        for b in &bucket_meta {
            terms.push(format!("\"{}\" ASC", b.alias)); // bucket alias = width_bucket index → numeric order
        }
        terms
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
        bucket_meta,
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
    fn snapshot_aggregate_measures() {
        let req: QueryRequest = serde_json::from_str(
            r#"{"measures":["count",{"avg":"attr.latency_ms"},{"p95":"attr.latency_ms"}],
                "dimensions":["app"],"time_range":{"last":"7d"}}"#,
        )
        .unwrap();
        insta::assert_snapshot!(build(&req, NOW).unwrap().sql);
    }

    #[test]
    fn snapshot_numeric_filter() {
        let req: QueryRequest = serde_json::from_str(
            r#"{"measures":["count"],
                "filters":[{"field":"attr.latency_ms","op":"gt","value":4242}],
                "time_range":{"last":"7d"}}"#,
        )
        .unwrap();
        let built = build(&req, NOW).unwrap();
        // the numeric filter value is bound, never spliced into SQL text
        assert!(!built.sql.contains("4242"));
        insta::assert_snapshot!(built.sql);
    }

    #[test]
    fn snapshot_numeric_bucket() {
        let req: QueryRequest = serde_json::from_str(
            r#"{"measures":["count","unique_installs"],
                "dimensions":[{"bucket":{"field":"attr.latency_ms","edges":[137,911]}}],
                "time_range":{"last":"7d"}}"#,
        )
        .unwrap();
        let built = build(&req, NOW).unwrap();
        assert!(!built.sql.contains("137") && !built.sql.contains("911")); // edges bound, not spliced
        assert_eq!(built.bucket_meta.len(), 1);
        assert_eq!(built.bucket_meta[0].labels, vec!["<137", "137-911", "911+"]);
        insta::assert_snapshot!(built.sql);
    }

    #[test]
    fn bucket_value_maps_index_to_label() {
        let labels = vec!["<50".to_string(), "50-200".to_string(), "200+".to_string()];
        assert_eq!(bucket_value(&labels, Some(1)), serde_json::json!("50-200"));
        assert_eq!(bucket_value(&labels, None), serde_json::Value::Null);
        assert_eq!(bucket_value(&labels, Some(9)), serde_json::Value::Null);
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
