use gauge_query::*;

#[test]
fn parses_spec_example_request() {
    let json = r#"{
      "measures": ["unique_installs"],
      "dimensions": ["app", "event_name"],
      "filters": [{"field": "app", "op": "eq", "value": "tome"},
                  {"field": "attr.surface", "op": "eq", "value": "mcp"}],
      "time_range": {"last": "7d"},
      "granularity": "day",
      "order": [{"field": "unique_installs", "dir": "desc"}],
      "limit": 100
    }"#;
    let req: QueryRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.measures, vec![Measure::UniqueInstalls]);
    assert_eq!(
        req.dimensions,
        vec![
            Dimension::Field(Field::App),
            Dimension::Field(Field::EventName)
        ]
    );
    assert_eq!(req.filters[1].field, Field::Attr("surface".into()));
    assert_eq!(req.granularity, Some(Granularity::Day));
    assert_eq!(req.limit, Some(100));
    validate(&req).unwrap();
}

#[test]
fn field_round_trips_attr_grammar() {
    assert_eq!(
        Field::parse("attr.latency_bucket").unwrap(),
        Field::Attr("latency_bucket".into())
    );
    assert_eq!(Field::Attr("x".into()).to_string(), "attr.x");
    assert!(Field::parse("attr.").is_err());
    assert!(Field::parse("attr.bad key").is_err());
    assert!(Field::parse("install_id").is_err()); // never queryable as a dimension
}

#[test]
fn unknown_top_level_field_rejected() {
    let json = r#"{"measures":["count"],"time_range":{"last":"1d"},"nope":1}"#;
    assert!(serde_json::from_str::<QueryRequest>(json).is_err());
}

#[test]
fn validate_rejects_empty_measures() {
    let req = QueryRequest {
        measures: vec![],
        ..minimal()
    };
    assert!(matches!(validate(&req), Err(QueryError::EmptyMeasures)));
}

#[test]
fn validate_rejects_limit_over_cap() {
    let req = QueryRequest {
        limit: Some(10_001),
        ..minimal()
    };
    assert!(matches!(
        validate(&req),
        Err(QueryError::LimitTooLarge(10_001))
    ));
}

#[test]
fn validate_rejects_bad_time_range() {
    for bad in ["7", "7w", "0d", "400d", "-1h"] {
        let req = QueryRequest {
            time_range: TimeRange::Last { last: bad.into() },
            ..minimal()
        };
        assert!(validate(&req).is_err(), "{bad} should be invalid");
    }
}

#[test]
fn validate_filter_op_value_rules() {
    // exists takes no value and only applies to attr fields
    let ok = Filter {
        field: Field::Attr("k".into()),
        op: FilterOp::Exists,
        value: None,
    };
    let req = QueryRequest {
        filters: vec![ok],
        ..minimal()
    };
    validate(&req).unwrap();

    let bad = Filter {
        field: Field::App,
        op: FilterOp::Exists,
        value: None,
    };
    let req = QueryRequest {
        filters: vec![bad],
        ..minimal()
    };
    assert!(validate(&req).is_err());

    let bad = Filter {
        field: Field::App,
        op: FilterOp::Eq,
        value: None,
    };
    let req = QueryRequest {
        filters: vec![bad],
        ..minimal()
    };
    assert!(validate(&req).is_err());

    let bad = Filter {
        field: Field::App,
        op: FilterOp::In,
        value: Some(FilterValue::One("x".into())),
    };
    let req = QueryRequest {
        filters: vec![bad],
        ..minimal()
    };
    assert!(validate(&req).is_err());
}

fn minimal() -> QueryRequest {
    QueryRequest {
        measures: vec![Measure::Count],
        dimensions: vec![],
        filters: vec![],
        time_range: TimeRange::Last { last: "1d".into() },
        granularity: None,
        order: vec![],
        limit: None,
    }
}
