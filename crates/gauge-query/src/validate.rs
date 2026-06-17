use thiserror::Error;
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

use crate::field::Field;
use crate::request::{
    Dimension, FilterOp, FilterValue, MAX_BUCKET_EDGES, MAX_LIMIT, QueryRequest, TimeRange,
};

#[derive(Debug, Error, PartialEq)]
pub enum QueryError {
    #[error("unknown field `{0}`")]
    UnknownField(String),
    #[error("measures must not be empty")]
    EmptyMeasures,
    #[error("limit {0} exceeds the maximum of {MAX_LIMIT}")]
    LimitTooLarge(u32),
    #[error("invalid time range `{0}` (expected <N>h, <N>d up to 365d, or RFC3339 from/to)")]
    BadTimeRange(String),
    #[error("filter on `{0}`: op `{1}` requires {2}")]
    BadFilter(String, String, &'static str),
    #[error("order field `{0}` is not in the selected output")]
    BadOrderField(String),
    #[error("numeric operation on `{0}` requires an attr.<key> field")]
    NumericFieldRequired(String),
    #[error(
        "bucket edges on `{0}` must be non-empty, finite, strictly ascending, and at most {MAX_BUCKET_EDGES}"
    )]
    BadBucketEdges(String),
    #[error("duplicate output `{0}` (two dimensions/measures resolve to the same column)")]
    DuplicateOutput(String),
}

/// Human-readable bucket labels for `width_bucket` indices 0..=edges.len().
/// e.g. `[50,200,500,1000]` → `["<50","50-200","200-500","500-1000","1000+"]`.
pub fn bucket_labels(edges: &[f64]) -> Vec<String> {
    fn fmt(x: f64) -> String {
        if x.fract() == 0.0 && x.abs() < 1e15 {
            format!("{}", x as i64)
        } else {
            format!("{x}")
        }
    }
    let mut labels = Vec::with_capacity(edges.len() + 1);
    if let Some(first) = edges.first() {
        labels.push(format!("<{}", fmt(*first)));
    }
    for w in edges.windows(2) {
        labels.push(format!("{}-{}", fmt(w[0]), fmt(w[1])));
    }
    if let Some(last) = edges.last() {
        labels.push(format!("{}+", fmt(*last)));
    }
    labels
}

pub fn parse_last(s: &str) -> Result<Duration, QueryError> {
    let bad = || QueryError::BadTimeRange(s.to_string());
    if s.len() < 2 {
        return Err(bad());
    }
    let (num, unit) = s.split_at(s.len() - 1);
    let n: i64 = num.parse().map_err(|_| bad())?;
    if n < 1 {
        return Err(bad());
    }
    let d = match unit {
        "h" => Duration::hours(n),
        "d" => Duration::days(n),
        _ => return Err(bad()),
    };
    if d > Duration::days(365) {
        return Err(bad());
    }
    Ok(d)
}

/// Resolve a TimeRange to concrete [from, to) bounds.
pub fn resolve_time_range(
    tr: &TimeRange,
    now: OffsetDateTime,
) -> Result<(OffsetDateTime, OffsetDateTime), QueryError> {
    match tr {
        TimeRange::Last { last } => Ok((now - parse_last(last)?, now)),
        TimeRange::Absolute { from, to } => {
            let f = OffsetDateTime::parse(from, &Rfc3339)
                .map_err(|_| QueryError::BadTimeRange(from.clone()))?;
            let t = OffsetDateTime::parse(to, &Rfc3339)
                .map_err(|_| QueryError::BadTimeRange(to.clone()))?;
            if f >= t {
                return Err(QueryError::BadTimeRange(format!("{from}..{to}")));
            }
            Ok((f, t))
        }
    }
}

pub fn validate(req: &QueryRequest) -> Result<(), QueryError> {
    if req.measures.is_empty() {
        return Err(QueryError::EmptyMeasures);
    }
    for m in &req.measures {
        if let Some(f) = m.numeric_field()
            && !matches!(f, Field::Attr(_))
        {
            return Err(QueryError::NumericFieldRequired(f.to_string()));
        }
    }
    use std::collections::HashSet;
    let mut seen: HashSet<String> = HashSet::new();
    for d in &req.dimensions {
        if let Dimension::Bucket { bucket } = d {
            if !matches!(bucket.field, Field::Attr(_)) {
                return Err(QueryError::NumericFieldRequired(bucket.field.to_string()));
            }
            let e = &bucket.edges;
            let ok = !e.is_empty()
                && e.len() <= MAX_BUCKET_EDGES
                && e.iter().all(|x| x.is_finite())
                && e.windows(2).all(|w| w[0] < w[1]);
            if !ok {
                return Err(QueryError::BadBucketEdges(bucket.field.to_string()));
            }
        }
        if !seen.insert(d.alias()) {
            return Err(QueryError::DuplicateOutput(d.alias()));
        }
    }
    for m in &req.measures {
        if !seen.insert(m.alias()) {
            return Err(QueryError::DuplicateOutput(m.alias()));
        }
    }
    if let Some(l) = req.limit
        && l > MAX_LIMIT
    {
        return Err(QueryError::LimitTooLarge(l));
    }
    resolve_time_range(&req.time_range, OffsetDateTime::now_utc())?;
    for f in &req.filters {
        let fname = f.field.to_string();
        let opname = format!("{:?}", f.op).to_lowercase();
        match (f.op, &f.value) {
            (FilterOp::Eq | FilterOp::Neq, Some(FilterValue::One(_))) => {}
            (FilterOp::In, Some(FilterValue::Many(v))) if !v.is_empty() => {}
            (FilterOp::Exists, None) => {
                if !matches!(f.field, Field::Attr(_)) {
                    return Err(QueryError::BadFilter(fname, opname, "an attr.<key> field"));
                }
            }
            (
                FilterOp::Gt | FilterOp::Gte | FilterOp::Lt | FilterOp::Lte,
                Some(FilterValue::Num(_)),
            ) => {
                if !matches!(f.field, Field::Attr(_)) {
                    return Err(QueryError::NumericFieldRequired(fname));
                }
            }
            (FilterOp::Gt | FilterOp::Gte | FilterOp::Lt | FilterOp::Lte, _) => {
                return Err(QueryError::BadFilter(
                    fname,
                    opname,
                    "a numeric value on an attr.<key> field",
                ));
            }
            (FilterOp::Eq | FilterOp::Neq, _) => {
                return Err(QueryError::BadFilter(
                    fname,
                    opname,
                    "a single string value",
                ));
            }
            (FilterOp::In, _) => {
                return Err(QueryError::BadFilter(
                    fname,
                    opname,
                    "a non-empty string array",
                ));
            }
            (FilterOp::Exists, Some(_)) => {
                return Err(QueryError::BadFilter(fname, opname, "no value"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::{Measure, QueryRequest};

    fn req_with(measures: Vec<Measure>) -> QueryRequest {
        QueryRequest {
            measures,
            dimensions: vec![],
            filters: vec![],
            time_range: crate::request::TimeRange::Last { last: "1d".into() },
            granularity: None,
            order: vec![],
            limit: None,
        }
    }

    #[test]
    fn aggregate_requires_attr_field() {
        let ok = req_with(vec![Measure::Avg(Field::Attr("latency_ms".into()))]);
        assert!(validate(&ok).is_ok());
        let bad = req_with(vec![Measure::Avg(Field::Os)]);
        assert!(matches!(
            validate(&bad),
            Err(QueryError::NumericFieldRequired(_))
        ));
    }

    #[test]
    fn bucket_edges_must_be_sorted_and_attr() {
        use crate::request::{BucketSpec, Dimension};
        let mut r = req_with(vec![Measure::Count]);
        r.dimensions = vec![Dimension::Bucket {
            bucket: BucketSpec {
                field: Field::Attr("latency_ms".into()),
                edges: vec![200.0, 50.0],
            },
        }];
        assert!(matches!(validate(&r), Err(QueryError::BadBucketEdges(_))));
        r.dimensions[0] = Dimension::Bucket {
            bucket: BucketSpec {
                field: Field::App,
                edges: vec![50.0, 200.0],
            },
        };
        assert!(matches!(
            validate(&r),
            Err(QueryError::NumericFieldRequired(_))
        ));
    }

    #[test]
    fn bucket_labels_span_edges() {
        let labels = crate::validate::bucket_labels(&[50.0, 200.0, 500.0, 1000.0]);
        assert_eq!(
            labels,
            vec!["<50", "50-200", "200-500", "500-1000", "1000+"]
        );
        // fractional edges keep their decimals
        assert_eq!(
            crate::validate::bucket_labels(&[1.5, 3.0]),
            vec!["<1.5", "1.5-3", "3+"]
        );
    }

    #[test]
    fn numeric_filter_requires_num_and_attr() {
        use crate::request::{Filter, FilterOp, FilterValue};
        let mut r = req_with(vec![Measure::Count]);
        r.filters = vec![Filter {
            field: Field::Attr("latency_ms".into()),
            op: FilterOp::Gt,
            value: Some(FilterValue::Num(500.0)),
        }];
        assert!(validate(&r).is_ok());
        // gt on a non-attr field is rejected
        r.filters[0].field = Field::Os;
        assert!(matches!(
            validate(&r),
            Err(QueryError::NumericFieldRequired(_))
        ));
        // gt with a string value is rejected
        r.filters[0].field = Field::Attr("latency_ms".into());
        r.filters[0].value = Some(FilterValue::One("500".into()));
        assert!(matches!(validate(&r), Err(QueryError::BadFilter(..))));
    }
}
