use std::path::PathBuf;

use crate::error::ClientError;

pub fn config_dir() -> Result<PathBuf, ClientError> {
    if let Ok(d) = std::env::var("GAUGE_CONFIG_DIR") {
        return Ok(PathBuf::from(d));
    }
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(xdg).join("gauge"));
    }
    std::env::var("HOME")
        .map(|h| PathBuf::from(h).join(".config").join("gauge"))
        .map_err(|_| ClientError::NoConfigDir)
}

pub fn config_path() -> Result<PathBuf, ClientError> {
    Ok(config_dir()?.join("config.toml"))
}

pub fn key_path(user_id: &str) -> Result<PathBuf, ClientError> {
    Ok(config_dir()?.join(format!("{user_id}.private")))
}

pub fn token_path() -> Result<PathBuf, ClientError> {
    Ok(config_dir()?.join("token.json"))
}
