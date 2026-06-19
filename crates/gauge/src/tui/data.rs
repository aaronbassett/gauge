use gauge_query::{
    BucketSpec, Dimension, Field, Granularity, Measure, QueryRequest, QueryResponse, TimeRange,
};

use crate::api::ApiClient;
use crate::error::ClientError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeWindow {
    H1,
    H24,
    D7,
    D30,
}

impl TimeWindow {
    pub fn next(self) -> Self {
        match self {
            Self::H1 => Self::H24,
            Self::H24 => Self::D7,
            Self::D7 => Self::D30,
            Self::D30 => Self::H1,
        }
    }
    pub fn last(&self) -> &'static str {
        match self {
            Self::H1 => "1h",
            Self::H24 => "24h",
            Self::D7 => "7d",
            Self::D30 => "30d",
        }
    }
    /// The relative range string for twice this window (for current-vs-previous deltas).
    pub fn doubled_last(&self) -> &'static str {
        match self {
            Self::H1 => "2h",
            Self::H24 => "48h",
            Self::D7 => "14d",
            Self::D30 => "60d",
        }
    }
    pub fn granularity(&self) -> Granularity {
        match self {
            Self::H1 | Self::H24 => Granularity::Hour,
            Self::D7 | Self::D30 => Granularity::Day,
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            Self::H1 => "last hour",
            Self::H24 => "last 24h",
            Self::D7 => "last 7d",
            Self::D30 => "last 30d",
        }
    }
}

fn base(w: TimeWindow) -> QueryRequest {
    QueryRequest {
        measures: vec![Measure::Count],
        dimensions: vec![],
        filters: vec![],
        time_range: TimeRange::Last {
            last: w.last().into(),
        },
        granularity: None,
        order: vec![],
        limit: None,
    }
}

/// Round x up to a "nice" 1/2/5×10ⁿ step (for readable histogram edges).
fn nice_round(x: f64) -> f64 {
    if x <= 0.0 {
        return 1.0;
    }
    let mag = 10f64.powf(x.log10().floor());
    let norm = x / mag; // 1.0..10.0
    let nice = if norm < 1.5 {
        1.0
    } else if norm < 3.0 {
        2.0
    } else if norm < 7.0 {
        5.0
    } else {
        10.0
    };
    nice * mag
}

/// ~6 nicely-rounded interior edges spanning [min, max] (→ up to 5 edges → 6 buckets).
pub fn derive_edges(min: f64, max: f64) -> Vec<f64> {
    if !min.is_finite() || !max.is_finite() || max <= min {
        return vec![nice_round(min.abs().max(1.0))];
    }
    let step = nice_round((max - min) / 6.0);
    let mut edges = Vec::new();
    let mut e = (min / step).ceil() * step;
    if e <= min {
        e += step;
    }
    while e < max && edges.len() < 5 {
        edges.push(e);
        e += step;
    }
    if edges.is_empty() {
        edges.push((min + max) / 2.0);
    }
    edges
}

fn numeric_attr_field(key: &str) -> Field {
    Field::parse(&format!("attr.{key}")).unwrap_or(Field::Attr(key.to_string()))
}

pub fn histogram_probe_request(w: TimeWindow, key: &str) -> QueryRequest {
    let f = numeric_attr_field(key);
    QueryRequest {
        measures: vec![Measure::Min(f.clone()), Measure::Max(f)],
        ..base(w)
    }
}

pub fn histogram_bucket_request(w: TimeWindow, key: &str, edges: Vec<f64>) -> QueryRequest {
    QueryRequest {
        measures: vec![Measure::Count],
        dimensions: vec![Dimension::Bucket {
            bucket: BucketSpec {
                field: numeric_attr_field(key),
                edges,
            },
        }],
        ..base(w)
    }
}

/// Probe min/max, derive edges, then fetch the bucketed counts.
pub async fn fetch_histogram(
    api: &ApiClient,
    w: TimeWindow,
    key: &str,
) -> Result<QueryResponse, ClientError> {
    let probe = api.query(&histogram_probe_request(w, key)).await?;
    let row = probe.rows.first().cloned().unwrap_or_default();
    let g = |k: &str| row.get(k).and_then(serde_json::Value::as_f64);
    // The probe response uses the output aliases produced by Measure::alias() in
    // gauge-query: Min(attr.<key>) → "min_<key>", Max(attr.<key>) → "max_<key>".
    // That is why format!("min_{key}") / format!("max_{key}") are the correct keys here.
    let (min, max) = (
        g(&format!("min_{key}")).unwrap_or(0.0),
        g(&format!("max_{key}")).unwrap_or(0.0),
    );
    // A degenerate range (all values equal, so min == max) causes derive_edges to return
    // a single split edge, yielding a valid two-bucket histogram rather than panicking.
    let edges = derive_edges(min, max);
    api.query(&histogram_bucket_request(w, key, edges)).await
}

use crate::tui::panels::{LabeledRequest, Panel, PanelCtx, ResultMap};

/// A source of query answers. `ApiClient` implements this against the server.
pub trait QuerySource {
    fn run(
        &self,
        req: &QueryRequest,
    ) -> impl std::future::Future<Output = Result<QueryResponse, String>> + Send;
}

impl QuerySource for ApiClient {
    async fn run(&self, req: &QueryRequest) -> Result<QueryResponse, String> {
        self.query(req).await.map_err(|e| e.to_string())
    }
}

/// Gather every visible panel's requests, deduplicated by key (first wins).
pub fn collect_requests(panels: &[Box<dyn Panel>], ctx: &PanelCtx) -> Vec<LabeledRequest> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for panel in panels {
        for lr in panel.data_requests(ctx) {
            if seen.insert(lr.key.clone()) {
                out.push(lr);
            }
        }
    }
    out
}

/// Run all requests concurrently, collecting them into a key→result map.
pub async fn fetch_all<Q: QuerySource>(q: &Q, requests: Vec<LabeledRequest>) -> ResultMap {
    let futs = requests.into_iter().map(|lr| async move {
        let result = q.run(&lr.request).await;
        (lr.key, result)
    });
    futures::future::join_all(futs).await.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn histogram_probe_then_bucket_request_shapes() {
        // the probe asks for min+max of the attr
        let probe = histogram_probe_request(TimeWindow::D7, "latency_ms");
        assert!(probe.measures.iter().any(|m| matches!(m, Measure::Min(_))));
        assert!(probe.measures.iter().any(|m| matches!(m, Measure::Max(_))));
        // the bucket request uses derived edges as a Dimension::Bucket
        let bucket = histogram_bucket_request(TimeWindow::D7, "latency_ms", vec![50.0, 200.0]);
        assert!(matches!(
            &bucket.dimensions[0],
            gauge_query::Dimension::Bucket { .. }
        ));
    }

    #[test]
    fn doubled_last_doubles_each_window() {
        assert_eq!(TimeWindow::H1.doubled_last(), "2h");
        assert_eq!(TimeWindow::H24.doubled_last(), "48h");
        assert_eq!(TimeWindow::D7.doubled_last(), "14d");
        assert_eq!(TimeWindow::D30.doubled_last(), "60d");
    }

    #[test]
    fn derive_edges_are_sorted_rounded_and_bounded() {
        let e = derive_edges(0.0, 1180.0);
        assert!(!e.is_empty() && e.len() <= 5);
        assert!(
            e.windows(2).all(|w| w[0] < w[1]),
            "edges must be strictly ascending"
        );
        // degenerate range still yields a usable single split
        let d = derive_edges(5.0, 5.0);
        assert_eq!(d.len(), 1);
    }
}

#[cfg(test)]
mod dash_tests {
    use super::*;
    use crate::tui::panels::{LabeledRequest, Panel, PanelCtx, ResultMap};
    use gauge_query::{QueryRequest, QueryResponse, TimeRange};

    fn req(last: &str) -> QueryRequest {
        QueryRequest {
            measures: vec![gauge_query::Measure::Count],
            dimensions: vec![],
            filters: vec![],
            time_range: TimeRange::Last { last: last.into() },
            granularity: None,
            order: vec![],
            limit: None,
        }
    }

    struct FakePanel(Vec<QueryRequest>);
    impl Panel for FakePanel {
        fn title(&self) -> String {
            "fake".into()
        }
        fn data_requests(&self, _c: &PanelCtx) -> Vec<LabeledRequest> {
            self.0.iter().cloned().map(LabeledRequest::new).collect()
        }
        fn render(
            &self,
            _f: &mut ratatui::Frame,
            _a: ratatui::layout::Rect,
            _c: &PanelCtx,
            _r: &ResultMap,
            _t: &crate::tui::theme::Theme,
        ) {
        }
    }

    struct FakeSource;
    impl QuerySource for FakeSource {
        async fn run(&self, r: &QueryRequest) -> Result<QueryResponse, String> {
            if matches!(&r.time_range, TimeRange::Last { last } if last == "boom") {
                return Err("kaboom".into());
            }
            Ok(QueryResponse {
                rows: vec![],
                truncated: false,
                elapsed_ms: 0,
                meta: None,
            })
        }
    }

    #[test]
    fn collect_requests_dedupes_identical_queries() {
        let panels: Vec<Box<dyn Panel>> = vec![
            Box::new(FakePanel(vec![req("1d"), req("7d")])),
            Box::new(FakePanel(vec![req("1d")])),
        ];
        let ctx = PanelCtx {
            window: TimeWindow::D7,
            filters: &[],
            meta: &[],
        };
        assert_eq!(collect_requests(&panels, &ctx).len(), 2);
    }

    #[tokio::test]
    async fn fetch_all_maps_results_and_errors_by_key() {
        let reqs = vec![
            LabeledRequest::new(req("1d")),
            LabeledRequest::new(req("boom")),
        ];
        let map = fetch_all(&FakeSource, reqs.clone()).await;
        assert_eq!(map.len(), 2);
        assert!(map.get(&reqs[0].key).unwrap().is_ok());
        assert!(map.get(&reqs[1].key).unwrap().is_err());
    }
}
