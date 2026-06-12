use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// The one error envelope: {code, message, remediation}.
/// Never put attribute values or request bodies in `message`.
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
    pub remediation: Option<String>,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self { status, code, message: message.into(), remediation: None }
    }
    pub fn with_remediation(mut self, r: impl Into<String>) -> Self {
        self.remediation = Some(r.into());
        self
    }
    pub fn bad_request(code: &'static str, m: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, m)
    }
    pub fn unauthorized(code: &'static str, m: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, code, m)
    }
    pub fn forbidden(code: &'static str, m: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, code, m)
    }
    pub fn not_found(code: &'static str, m: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, code, m)
    }
    pub fn unprocessable(code: &'static str, m: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, code, m)
    }
    pub fn service_unavailable(code: &'static str, m: impl Into<String>) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, code, m)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = serde_json::json!({
            "code": self.code,
            "message": self.message,
            "remediation": self.remediation,
        });
        (self.status, Json(body)).into_response()
    }
}
