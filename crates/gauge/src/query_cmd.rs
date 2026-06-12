use gauge_query::QueryRequest;

use crate::api::ApiClient;
use crate::error::ClientError;

pub fn parse_request(json: &str) -> Result<QueryRequest, ClientError> {
    Ok(serde_json::from_str(json)?)
}

pub async fn run(api: &ApiClient, request_json: &str) -> Result<String, ClientError> {
    let req = parse_request(request_json)?;
    let resp = api.query(&req).await?;
    Ok(serde_json::to_string_pretty(&resp)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_request_json_with_helpful_error() {
        let err = parse_request(r#"{"measures":["count"]}"#).unwrap_err();
        assert!(err.to_string().contains("time_range"));
        let err = parse_request("not json").unwrap_err();
        assert!(err.to_string().contains("expected"));
    }

    #[test]
    fn accepts_valid_request() {
        parse_request(r#"{"measures":["count"],"time_range":{"last":"1d"}}"#).unwrap();
    }
}
