use axum::extract::{Request, State};
use axum::http::header::AUTHORIZATION;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use gauge_auth::{Role, verify_token};

use crate::error::ApiError;
use crate::state::AppState;

#[derive(Debug, Clone)]
pub struct AuthContext {
    pub sub: String,
    pub role: Role,
    pub jti: String,
}

pub async fn require_bearer(State(st): State<AppState>, mut req: Request, next: Next) -> Response {
    let token = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    let Some(token) = token else {
        return ApiError::unauthorized("missing_token", "missing Authorization: Bearer header")
            .with_remediation("run `gauge login`")
            .into_response();
    };
    match verify_token(&st.secret, token) {
        Ok(claims) => {
            req.extensions_mut().insert(AuthContext {
                sub: claims.sub,
                role: claims.role,
                jti: claims.jti,
            });
            next.run(req).await
        }
        Err(_) => ApiError::unauthorized("invalid_token", "token is invalid or expired")
            .with_remediation("run `gauge login`")
            .into_response(),
    }
}
