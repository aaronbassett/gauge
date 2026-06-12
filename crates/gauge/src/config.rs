use crate::error::ClientError;
use crate::paths;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ClientConfig {
    pub server_url: String,
    pub user_id: String,
}

impl ClientConfig {
    pub fn load() -> Result<Self, ClientError> {
        let path = paths::config_path()?;
        let raw = std::fs::read_to_string(&path).map_err(|_| ClientError::ConfigMissing(path))?;
        let mut cfg: ClientConfig =
            toml::from_str(&raw).map_err(|e| ClientError::ConfigInvalid(e.to_string()))?;
        while cfg.server_url.ends_with('/') {
            cfg.server_url.pop();
        }
        Ok(cfg)
    }
}
