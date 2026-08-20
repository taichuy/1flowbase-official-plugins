use anyhow::{bail, Context, Result};
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue},
    Method, Url,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use crate::{
    build_base_headers, build_http_client, inject_provider_auth,
    provider_upstream_error_from_response, sanitize_reqwest_error, ProviderConfig,
};

const CHATGPT_USAGE_WINDOW_SECONDS: [u64; 2] = [18_000, 604_800];

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProviderResetCreditRuntimeInput {
    #[serde(default)]
    pub(super) provider_config: Value,
    pub(super) operation: ProviderResetCreditOperation,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum ProviderResetCreditOperation {
    Count,
    Consume { idempotency_key: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProviderUsageWindow {
    limit_window_seconds: u64,
    used_percent: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    reset_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProviderUsageWindowsResult {
    windows: Vec<ProviderUsageWindow>,
    queried_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum ProviderResetCreditResult {
    Count { available_count: u32 },
    Consumed,
}

#[derive(Debug, Deserialize)]
struct WhamUsagePayload {
    rate_limit: Option<WhamRateLimit>,
}

#[derive(Debug, Deserialize)]
struct WhamRateLimit {
    primary_window: Option<WhamUsageWindow>,
    secondary_window: Option<WhamUsageWindow>,
}

#[derive(Debug, Deserialize)]
struct WhamUsageWindow {
    used_percent: f64,
    limit_window_seconds: u64,
    #[serde(default)]
    reset_at: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct WhamResetCreditCount {
    available_count: u64,
}

pub(super) async fn get_usage_windows(
    config: &ProviderConfig,
) -> Result<ProviderUsageWindowsResult> {
    let payload = request_wham_json(config, "usage", Method::GET, None).await?;
    project_usage_windows(payload)
}

pub(super) async fn reset_credit(
    config: &ProviderConfig,
    operation: ProviderResetCreditOperation,
) -> Result<ProviderResetCreditResult> {
    match operation {
        ProviderResetCreditOperation::Count => {
            let payload =
                request_wham_json(config, "rate-limit-reset-credits", Method::GET, None).await?;
            Ok(ProviderResetCreditResult::Count {
                available_count: project_reset_credit_count(payload)?,
            })
        }
        ProviderResetCreditOperation::Consume { idempotency_key } => {
            if idempotency_key.trim().is_empty() {
                bail!("reset credit consume requires a non-empty idempotency key");
            }
            // Do not retry this request: a transport failure leaves the upstream consumption
            // outcome unknown, so only the caller's logical-attempt key may be replayed later.
            let payload = request_wham_json(
                config,
                "rate-limit-reset-credits/consume",
                Method::POST,
                Some(json!({ "redeem_request_id": idempotency_key })),
            )
            .await?;
            project_reset_credit_consume(payload)
        }
    }
}

fn project_usage_windows(payload: Value) -> Result<ProviderUsageWindowsResult> {
    let payload: WhamUsagePayload =
        serde_json::from_value(payload).context("ChatGPT /wham/usage response is invalid")?;
    let rate_limit = payload
        .rate_limit
        .context("ChatGPT /wham/usage response does not include rate_limit")?;
    let windows = [rate_limit.primary_window, rate_limit.secondary_window]
        .into_iter()
        .flatten()
        .map(project_usage_window)
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    if windows.is_empty() {
        bail!("ChatGPT /wham/usage response has no 5h or 7d rate-limit window");
    }
    Ok(ProviderUsageWindowsResult {
        windows,
        queried_at: OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .context("formatting ChatGPT usage query timestamp")?,
    })
}

fn project_usage_window(window: WhamUsageWindow) -> Result<Option<ProviderUsageWindow>> {
    if !window.used_percent.is_finite() || !(0.0..=100.0).contains(&window.used_percent) {
        bail!("ChatGPT /wham/usage has an invalid used_percent");
    }
    if window.limit_window_seconds == 0 {
        bail!("ChatGPT /wham/usage has a zero limit_window_seconds");
    }
    if !CHATGPT_USAGE_WINDOW_SECONDS.contains(&window.limit_window_seconds) {
        return Ok(None);
    }
    Ok(Some(ProviderUsageWindow {
        limit_window_seconds: window.limit_window_seconds,
        used_percent: window.used_percent,
        reset_at: format_reset_at(window.reset_at)?,
    }))
}

fn format_reset_at(raw: Option<Value>) -> Result<Option<String>> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    match raw {
        Value::Null => Ok(None),
        Value::Number(value) => {
            let timestamp = value
                .as_i64()
                .context("ChatGPT /wham/usage reset_at must be a Unix timestamp")?;
            Ok(Some(
                OffsetDateTime::from_unix_timestamp(timestamp)
                    .context("ChatGPT /wham/usage reset_at is out of range")?
                    .format(&Rfc3339)
                    .context("formatting ChatGPT usage reset_at")?,
            ))
        }
        Value::String(value) => {
            let timestamp = OffsetDateTime::parse(&value, &Rfc3339)
                .context("ChatGPT /wham/usage reset_at must be RFC3339")?;
            Ok(Some(
                timestamp
                    .format(&Rfc3339)
                    .context("formatting ChatGPT usage reset_at")?,
            ))
        }
        _ => bail!("ChatGPT /wham/usage reset_at has an invalid type"),
    }
}

fn project_reset_credit_count(payload: Value) -> Result<u32> {
    let payload: WhamResetCreditCount = serde_json::from_value(payload)
        .context("ChatGPT reset-credit count response is invalid")?;
    u32::try_from(payload.available_count).context("ChatGPT reset-credit count exceeds u32")
}

fn project_reset_credit_consume(payload: Value) -> Result<ProviderResetCreditResult> {
    match payload.get("code").and_then(Value::as_str) {
        Some("reset") => Ok(ProviderResetCreditResult::Consumed),
        Some(code) => bail!("ChatGPT reset credit was not consumed: {code}"),
        None => bail!("ChatGPT reset-credit consume response is missing code"),
    }
}

async fn request_wham_json(
    config: &ProviderConfig,
    endpoint: &str,
    method: Method,
    body: Option<Value>,
) -> Result<Value> {
    let client = build_http_client(config)?;
    let mut request = client
        .request(method, wham_endpoint_url(config, endpoint)?)
        .headers(build_wham_headers(config, body.is_some())?);
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request
        .send()
        .await
        .map_err(|error| sanitize_reqwest_error(error, config))?;
    if !response.status().is_success() {
        return Err(provider_upstream_error_from_response(response)
            .await?
            .into());
    }
    let text = response.text().await?;
    serde_json::from_str(&text).context("ChatGPT /wham response is not JSON")
}

fn wham_endpoint_url(config: &ProviderConfig, endpoint: &str) -> Result<String> {
    let mut url = Url::parse(config.base_url.trim_end_matches('/'))
        .with_context(|| format!("invalid base_url: {}", config.base_url))?;
    let base_path = url.path().trim_end_matches('/');
    let backend_path = base_path.strip_suffix("/codex").unwrap_or(base_path);
    url.set_path(&format!("{backend_path}/wham/{endpoint}"));
    url.set_query(None);
    Ok(url.to_string())
}

fn build_wham_headers(config: &ProviderConfig, include_json_body: bool) -> Result<HeaderMap> {
    let mut headers = build_base_headers(config, include_json_body, "application/json")?;
    headers.insert(
        HeaderName::from_static("openai-beta"),
        HeaderValue::from_static("codex-1"),
    );
    headers.insert(
        HeaderName::from_static("oai-language"),
        HeaderValue::from_static("en-US"),
    );
    inject_provider_auth(&mut headers, config)?;
    Ok(headers)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        project_reset_credit_consume, project_reset_credit_count, project_usage_windows,
        wham_endpoint_url, ProviderResetCreditResult,
    };
    use crate::normalize_provider_config;

    #[test]
    fn usage_projects_duration_normalized_windows_without_upstream_labels() {
        let usage = project_usage_windows(json!({
            "rate_limit": {
                "primary_window": {
                    "used_percent": 0.0,
                    "limit_window_seconds": 18_000,
                    "reset_at": 1_770_000_000
                },
                "secondary_window": {
                    "used_percent": 100.0,
                    "limit_window_seconds": 604_800,
                    "reset_at": null
                }
            }
        }))
        .expect("known ChatGPT windows should project");

        assert_eq!(usage.windows.len(), 2);
        assert_eq!(usage.windows[0].limit_window_seconds, 18_000);
        assert_eq!(usage.windows[0].used_percent, 0.0);
        assert!(usage.windows[0].reset_at.is_some());
        assert_eq!(usage.windows[1].limit_window_seconds, 604_800);
        assert_eq!(usage.windows[1].used_percent, 100.0);
        assert_eq!(usage.windows[1].reset_at, None);
    }

    #[test]
    fn usage_accepts_a_null_window_without_inventing_a_snapshot() {
        let usage = project_usage_windows(json!({
            "rate_limit": {
                "primary_window": null,
                "secondary_window": {
                    "used_percent": 42.0,
                    "limit_window_seconds": 604_800,
                    "reset_at": "2026-08-20T10:00:00Z"
                }
            }
        }))
        .expect("a remaining valid window should stay observable");

        assert_eq!(usage.windows.len(), 1);
        assert_eq!(usage.windows[0].limit_window_seconds, 604_800);
    }

    #[test]
    fn reset_credit_projection_hides_credit_detail_and_accepts_only_reset() {
        assert_eq!(
            project_reset_credit_count(json!({
                "available_count": 2,
                "credits": [{ "id": "upstream-private-credit" }]
            }))
            .expect("count should use only available_count"),
            2
        );
        assert!(matches!(
            project_reset_credit_consume(json!({ "code": "reset", "credit": { "id": "hidden" } }))
                .expect("reset response should be consumed"),
            ProviderResetCreditResult::Consumed
        ));
        assert!(project_reset_credit_consume(json!({ "code": "no_credit" })).is_err());
        assert!(project_reset_credit_consume(json!({ "code": "already_redeemed" })).is_err());
    }

    #[test]
    fn wham_endpoint_is_sibling_to_the_codex_responses_base_url() {
        let config = normalize_provider_config(&json!({
            "base_url": "https://chatgpt.com/backend-api/codex",
            "access_token": "test-token"
        }))
        .expect("fixture config should be valid");

        assert_eq!(
            wham_endpoint_url(&config, "rate-limit-reset-credits/consume")
                .expect("usage URL should resolve"),
            "https://chatgpt.com/backend-api/wham/rate-limit-reset-credits/consume"
        );
    }
}
