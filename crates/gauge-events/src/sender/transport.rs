use thiserror::Error;

use crate::otlp::ExportLogsServiceRequest;

#[derive(Debug, Error)]
pub enum SenderError {
    #[error("endpoint must use https (plain http allowed only for loopback)")]
    InsecureEndpoint,
    #[error("http error: {0}")]
    Http(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub fn endpoint_allowed(endpoint: &str) -> bool {
    endpoint.starts_with("https://")
        || endpoint.starts_with("http://127.0.0.1")
        || endpoint.starts_with("http://localhost")
}

/// Blocking POST to {endpoint}/v1/logs. 5s timeout, no redirects (fail-closed).
pub fn post_batch(endpoint: &str, req: &ExportLogsServiceRequest) -> Result<u16, SenderError> {
    if !endpoint_allowed(endpoint) {
        return Err(SenderError::InsecureEndpoint);
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| SenderError::Http(e.to_string()))?;
    let resp = client
        .post(format!("{endpoint}/v1/logs"))
        .json(req)
        .send()
        .map_err(|e| SenderError::Http(e.to_string()))?;
    Ok(resp.status().as_u16())
}
