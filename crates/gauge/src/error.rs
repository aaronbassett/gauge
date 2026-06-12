use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("could not determine a config directory (set GAUGE_CONFIG_DIR, XDG_CONFIG_HOME, or HOME)")]
    NoConfigDir,
    #[error("missing config file {0} — create it with server_url and user_id")]
    ConfigMissing(PathBuf),
    #[error("invalid config: {0}")]
    ConfigInvalid(String),
    #[error("no private key at {0} — run `gauge keys generate --user-id <id>`")]
    KeyMissing(PathBuf),
    #[error("refusing to overwrite existing key at {0}")]
    KeyExists(PathBuf),
    #[error("auth error: {0} — run `gauge login`")]
    Auth(#[from] gauge_auth::AuthError),
    #[error("http error: {0}")]
    Http(String),
    #[error("server error {status} ({code}): {message}{}", remediation.as_deref().map(|r| format!(" — {r}")).unwrap_or_default())]
    Api { status: u16, code: String, message: String, remediation: Option<String> },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
