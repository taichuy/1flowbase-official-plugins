use serde::Deserialize;
use url::Url;

use crate::{Failure, Result};

// Credentials have no Debug implementation and are never returned to the public UI.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    pub client_id: String,
    pub client_secret: String,
    pub callback_url: String,
    #[serde(default)]
    pub self_registration_enabled: bool,
    #[serde(default)]
    pub minimum_account_age_years: u32,
}
impl Config {
    pub fn validate(&self) -> Result<()> {
        if self.client_id.trim().is_empty() || self.client_secret.trim().is_empty() {
            return Err(Failure("github_config_invalid"));
        }
        let url = Url::parse(&self.callback_url).map_err(|_| Failure("github_config_invalid"))?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(Failure("github_config_invalid"));
        }
        Ok(())
    }
    pub fn validate_callback(&self, callback: &str) -> Result<()> {
        if callback != self.callback_url {
            return Err(Failure("github_callback_mismatch"));
        }
        Ok(())
    }
}
