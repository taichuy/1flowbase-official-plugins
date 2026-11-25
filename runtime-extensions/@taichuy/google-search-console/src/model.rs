use crate::{pkce::sha256, Failure, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) const CONFIG_ID: &str = "default";
pub(crate) const SCOPE_READONLY: &str = "https://www.googleapis.com/auth/webmasters.readonly";
pub(crate) const SCOPE_FULL: &str = "https://www.googleapis.com/auth/webmasters";

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ScopeMode {
    Readonly,
    Full,
}

impl ScopeMode {
    pub fn parse(raw: &str) -> Result<Self> {
        match raw {
            "readonly" => Ok(Self::Readonly),
            "full" => Ok(Self::Full),
            _ => Err(Failure("invalid_input")),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Readonly => "readonly",
            Self::Full => "full",
        }
    }

    pub fn oauth_scope_string(self) -> String {
        let gsc = match self {
            Self::Readonly => SCOPE_READONLY,
            Self::Full => SCOPE_FULL,
        };
        format!("openid email profile {gsc}")
    }

    pub fn from_granted_scopes(scopes: &[String]) -> Self {
        if scopes.iter().any(|s| s == SCOPE_FULL) {
            Self::Full
        } else {
            Self::Readonly
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct OAuthConfig {
    pub config_id: String,
    pub auth_mode: String,
    pub client_id: String,
    pub credential_id: String,
    pub default_scope_mode: ScopeMode,
    pub updated_at: String,
}

impl OAuthConfig {
    pub fn public(&self) -> Value {
        json!({
            "configured": true,
            "auth_mode": self.auth_mode,
            "client_id": self.client_id,
            "has_client_secret": !self.credential_id.is_empty(),
            "default_scope_mode": self.default_scope_mode.as_str(),
            "updated_at": self.updated_at,
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Account {
    pub account_id: String,
    pub google_sub: String,
    pub email: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub scope_mode: ScopeMode,
    pub granted_scopes: Vec<String>,
    pub enabled: bool,
    pub status: String,
    pub last_error: Option<String>,
    pub credential_id: String,
    pub site_count: u64,
    pub last_synced_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl Account {
    pub fn summary(&self) -> Value {
        json!({
            "account_id": self.account_id,
            "email": self.email,
            "display_name": self.display_name,
            "avatar_url": self.avatar_url,
            "scope_mode": self.scope_mode.as_str(),
            "granted_scopes": self.granted_scopes,
            "enabled": self.enabled,
            "status": self.status,
            "last_error": self.last_error,
            "has_credential": !self.credential_id.is_empty(),
            "site_count": self.site_count,
            "last_synced_at": self.last_synced_at,
            "created_at": self.created_at,
            "updated_at": self.updated_at,
        })
    }

    pub fn token_cache_key(&self) -> String {
        format!("gsc-at:{}:{}", self.account_id, self.credential_id)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Site {
    pub site_record_id: String,
    pub account_id: String,
    pub site_url: String,
    pub property_type: String,
    pub permission_level: String,
    pub sitemap_count: Option<u64>,
    pub last_synced_at: String,
}

impl Site {
    pub fn summary(&self) -> Value {
        json!({
            "site_record_id": self.site_record_id,
            "account_id": self.account_id,
            "site_url": self.site_url,
            "property_type": self.property_type,
            "permission_level": self.permission_level,
            "sitemap_count": self.sitemap_count,
            "last_synced_at": self.last_synced_at,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct OAuthPendingState {
    pub actor_id: String,
    pub workspace_id: String,
    pub code_verifier: String,
    pub redirect_uri: String,
    pub scope_mode: ScopeMode,
    pub reauth_account_id: Option<String>,
    pub expires_at_unix_ms: i64,
}

#[derive(Clone, Debug)]
pub(crate) struct CallContext {
    pub actor_id: String,
    pub workspace_id: String,
    #[allow(dead_code)]
    pub deadline_unix_ms: i64,
}

impl CallContext {
    pub fn from_request(request: &Value) -> Result<Self> {
        let context = &request["input"]["call_context"];
        let text = |key: &str| {
            context[key]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .ok_or(Failure("call_context_required"))
        };
        let deadline_unix_ms = context["deadline_unix_ms"]
            .as_i64()
            .filter(|deadline| *deadline > now_unix_ms())
            .ok_or(Failure("call_context_required"))?;
        Ok(Self {
            actor_id: text("actor_id")?,
            workspace_id: text("workspace_id")?,
            deadline_unix_ms,
        })
    }
}

pub(crate) fn deterministic_uuid(namespace: &str, key: &str) -> String {
    let mut input = Vec::with_capacity(namespace.len() + 1 + key.len());
    input.extend_from_slice(namespace.as_bytes());
    input.push(b':');
    input.extend_from_slice(key.as_bytes());
    let digest = sha256(&input);
    let mut b = [0u8; 16];
    b.copy_from_slice(&digest[..16]);
    b[6] = (b[6] & 0x0f) | 0x50;
    b[8] = (b[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

pub(crate) fn deterministic_account_id(google_sub: &str) -> String {
    deterministic_uuid("1flowbase:gsc:account:v1", google_sub)
}

pub(crate) fn deterministic_site_record_id(account_id: &str, site_url: &str) -> String {
    deterministic_uuid("1flowbase:gsc:site:v1", &format!("{account_id}
{site_url}"))
}

pub(crate) fn property_type_for_site_url(site_url: &str) -> String {
    if site_url.starts_with("sc-domain:") {
        "domain".to_owned()
    } else {
        "url_prefix".to_owned()
    }
}

pub(crate) fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}

pub(crate) fn now_unix_secs() -> i64 {
    now_unix_ms().div_euclid(1000)
}

/// Howard Hinnant civil-from-days algorithm in UTC.
pub(crate) fn format_utc_datetime(unix_secs: i64) -> String {
    let days = unix_secs.div_euclid(86_400);
    let secs = unix_secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = secs / 3600;
    let minute = (secs % 3600) / 60;
    let second = secs % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

pub(crate) fn format_utc_date(unix_secs: i64) -> String {
    let days = unix_secs.div_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

pub(crate) fn now_rfc3339() -> String {
    format_utc_datetime(now_unix_secs())
}

pub(crate) fn utc_date_days_ago(days_ago: i64) -> String {
    format_utc_date(now_unix_secs().saturating_sub(days_ago.saturating_mul(86_400)))
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + i64::from(m <= 2);
    (year, m as u32, d as u32)
}
