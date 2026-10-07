use crate::source::{atomic_write, reject_symlink_path, CheckpointLock};
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Config is private installation data. Never print/debug it: it contains a credential.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    pub endpoint: String,
    pub source_path: PathBuf,
    pub state_path: PathBuf,
    pub api_key: String,
}
impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        reject_symlink_path(path)?;
        let bytes = fs::read(path).context("Cannot read collector config")?;
        let config: Self = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("Invalid collector config"))?;
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "Unsupported config version");
        validate_endpoint(&self.endpoint)?;
        ensure!(
            !self.api_key.trim().is_empty() && !self.api_key.contains(['\r', '\n']),
            "Invalid API key"
        );
        ensure!(
            self.source_path.is_absolute() && self.state_path.is_absolute(),
            "Config paths must be absolute"
        );
        reject_symlink_path(&self.source_path)?;
        reject_symlink_path(&self.state_path)?;
        Ok(())
    }
}
fn validate_endpoint(endpoint: &str) -> Result<()> {
    let url = reqwest::Url::parse(endpoint).map_err(|_| anyhow::anyhow!("Invalid endpoint URL"))?;
    ensure!(
        matches!(url.scheme(), "http" | "https") && url.host_str().is_some(),
        "Endpoint must be HTTP(S)"
    );
    ensure!(
        url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "Endpoint must not contain credentials, query or fragment"
    );
    Ok(())
}
pub fn absolute(path: &Path) -> Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    // Normalize without requiring source/config to exist. This also binds reruns consistently.
    let mut normalized = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    Ok(normalized)
}
/// Reconfiguration rotates the key but preserves source/state identity. Source or endpoint
/// changes require an explicitly separate config rather than silently rebinding a checkpoint.
pub fn configure(
    path: &Path,
    endpoint: String,
    source_path: &Path,
    api_key: String,
) -> Result<Config> {
    let path = absolute(path)?;
    reject_symlink_path(&path)?;
    let source_path = absolute(source_path)?;
    let state_path = path
        .parent()
        .context("Config requires a parent directory")?
        .join("state.json");
    let _lock = CheckpointLock::acquire(&state_path)?;
    let existing = match fs::read(&path) {
        Ok(bytes) => Some(
            serde_json::from_slice::<Config>(&bytes)
                .map_err(|_| anyhow::anyhow!("Invalid existing config"))?,
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => bail!("Cannot read existing config"),
    };
    let state_path = if let Some(old) = existing {
        old.validate()?;
        ensure!(
            old.endpoint == endpoint
                && old.source_path == source_path
                && old.state_path == state_path,
            "Config identity differs; use a separate config directory"
        );
        old.state_path
    } else {
        state_path
    };
    let config = Config {
        version: 1,
        endpoint,
        source_path,
        state_path,
        api_key,
    };
    config.validate()?;
    atomic_write(
        &path,
        &serde_json::to_vec(&config).context("Cannot encode config")?,
    )?;
    Ok(config)
}
