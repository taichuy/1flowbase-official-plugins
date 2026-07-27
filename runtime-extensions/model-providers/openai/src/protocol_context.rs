use std::collections::{BTreeMap, BTreeSet};

use anyhow::{bail, Context, Result};
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue},
    Url,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const OPENAI_CHAT_TYPED_BODY_FIELDS: &[&str] = &[
    "model",
    "messages",
    "stream",
    "user",
    "metadata",
    "max_completion_tokens",
    "max_tokens",
    "audio",
    "modalities",
    "tools",
    "tool_choice",
    "function_call",
    "parallel_tool_calls",
    "response_format",
    "reasoning_effort",
    "temperature",
    "top_p",
    "presence_penalty",
    "frequency_penalty",
    "seed",
    "stop",
    "stream_options",
];
const OPENAI_RESPONSES_TYPED_BODY_FIELDS: &[&str] = &[
    "model",
    "input",
    "instructions",
    "stream",
    "user",
    "metadata",
    "max_output_tokens",
    "store",
    "previous_response_id",
    "tools",
    "tool_choice",
    "parallel_tool_calls",
    "response_format",
    "text",
    "reasoning",
    "background",
    "include",
    "prompt_cache_key",
    "client_metadata",
    "max_tool_calls",
    "truncation",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ProtocolContextEnvelope {
    pub source_protocol: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub query: BTreeMap<String, Vec<String>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, Vec<String>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub body: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpenAiWireProtocol {
    Chat,
    Responses,
}

impl OpenAiWireProtocol {
    pub(crate) fn source_protocol(self) -> &'static str {
        match self {
            Self::Chat => "openai_chat",
            Self::Responses => "openai_responses",
        }
    }

    fn typed_body_fields(self) -> &'static [&'static str] {
        match self {
            Self::Chat => OPENAI_CHAT_TYPED_BODY_FIELDS,
            Self::Responses => OPENAI_RESPONSES_TYPED_BODY_FIELDS,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct RestoredProtocolContext {
    query: BTreeMap<String, Vec<String>>,
    headers: BTreeMap<String, Vec<String>>,
}

pub(crate) fn restore_protocol_context(
    protocol: OpenAiWireProtocol,
    mut typed_body: Value,
    envelope: Option<&ProtocolContextEnvelope>,
) -> Result<(Value, RestoredProtocolContext)> {
    let Some(envelope) = envelope else {
        return Ok((typed_body, RestoredProtocolContext::default()));
    };
    if envelope.source_protocol != protocol.source_protocol() {
        bail!(
            "unconsumed foreign protocol context: expected {}, got {}",
            protocol.source_protocol(),
            envelope.source_protocol
        );
    }

    validate_multi_value_fields("query", &envelope.query, false)?;
    validate_multi_value_fields("headers", &envelope.headers, true)?;
    let body = typed_body
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("typed OpenAI request body must be an object"))?;
    for (name, value) in &envelope.body {
        validate_protocol_field_name("body", name)?;
        validate_protocol_body_value(value, &format!("body.{name}"))?;
        if protocol.typed_body_fields().contains(&name.as_str()) {
            bail!("protocol context body collides with typed field: {name}");
        }
        match body.get(name) {
            Some(existing) if existing == value => {}
            Some(_) => bail!("protocol context body collides with an owned field: {name}"),
            None => {
                body.insert(name.clone(), value.clone());
            }
        }
    }

    Ok((
        typed_body,
        RestoredProtocolContext {
            query: envelope.query.clone(),
            headers: envelope.headers.clone(),
        },
    ))
}

pub(crate) fn append_protocol_query(
    url: &mut Url,
    context: &RestoredProtocolContext,
) -> Result<()> {
    let owned = url
        .query_pairs()
        .map(|(name, _)| name.into_owned())
        .collect::<BTreeSet<_>>();
    for name in context.query.keys() {
        if owned.contains(name) {
            bail!("protocol context query collides with an owned field: {name}");
        }
    }
    let mut query = url.query_pairs_mut();
    for (name, values) in &context.query {
        for value in values {
            query.append_pair(name, value);
        }
    }
    Ok(())
}

pub(crate) fn append_protocol_headers(
    headers: &mut HeaderMap,
    context: &RestoredProtocolContext,
) -> Result<()> {
    for (name, values) in &context.headers {
        let header_name = HeaderName::from_bytes(name.as_bytes())
            .with_context(|| format!("invalid protocol context header name: {name}"))?;
        if headers.contains_key(&header_name) {
            bail!("protocol context header collides with an owned field: {name}");
        }
        for value in values {
            headers.append(
                header_name.clone(),
                HeaderValue::from_str(value)
                    .with_context(|| format!("invalid protocol context header value: {name}"))?,
            );
        }
    }
    Ok(())
}

fn validate_multi_value_fields(
    location: &str,
    fields: &BTreeMap<String, Vec<String>>,
    headers: bool,
) -> Result<()> {
    for (name, values) in fields {
        validate_protocol_field_name(location, name)?;
        if values.is_empty() {
            bail!("protocol context {location} field has no values: {name}");
        }
        if headers {
            validate_protocol_header_name(name)?;
            HeaderName::from_bytes(name.as_bytes())
                .with_context(|| format!("invalid protocol context header name: {name}"))?;
            for value in values {
                HeaderValue::from_str(value)
                    .with_context(|| format!("invalid protocol context header value: {name}"))?;
            }
        }
    }
    Ok(())
}

fn validate_protocol_header_name(name: &str) -> Result<()> {
    let normalized = name.trim().to_ascii_lowercase().replace('_', "-");
    if matches!(
        normalized.as_str(),
        "content-type"
            | "accept"
            | "accept-encoding"
            | "accept-language"
            | "origin"
            | "x-codex-turn-metadata"
    ) || normalized.starts_with("sec-websocket-")
    {
        bail!("reserved protocol context field at headers: {name}");
    }
    Ok(())
}

fn validate_protocol_body_value(value: &Value, path: &str) -> Result<()> {
    match value {
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                validate_protocol_body_value(value, &format!("{path}[{index}]"))?;
            }
        }
        Value::Object(object) => {
            for (name, value) in object {
                validate_protocol_field_name(path, name)?;
                validate_protocol_body_value(value, &format!("{path}.{name}"))?;
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
    Ok(())
}

fn validate_protocol_field_name(location: &str, name: &str) -> Result<()> {
    if protocol_context_field_is_safe(name) {
        return Ok(());
    }
    bail!("reserved protocol context field at {location}: {name}")
}

fn protocol_context_field_is_safe(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    if lower.is_empty() || lower.starts_with("__") {
        return false;
    }
    let normalized = lower.replace('_', "-");
    !matches!(
        normalized.as_str(),
        "auth"
            | "authentication"
            | "authentication-info"
            | "authorization"
            | "x-authorization"
            | "proxy-authorization"
            | "proxy-authenticate"
            | "proxy-authentication-info"
            | "www-authenticate"
            | "x-api-key"
            | "api-key"
            | "x-auth-token"
            | "auth-token"
            | "bearer-token"
            | "x-access-token"
            | "access-token"
            | "refresh-token"
            | "id-token"
            | "client-secret"
            | "api-secret"
            | "password"
            | "passwd"
            | "x-csrf-token"
            | "x-xsrf-token"
            | "csrf-token"
            | "cookie"
            | "set-cookie"
            | "host"
            | "connection"
            | "proxy-connection"
            | "keep-alive"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "content-length"
            | "client-protocol-envelope"
            | "native-model-prompt-context"
            | "native-model-request-context"
            | "native-transport"
            | "provider-transport"
            | "request-context"
            | "run-context"
            | "trace-context"
            | "compatibility-mode"
            | "sys"
            | "env"
            | "trigger"
            | "forwarded"
            | "via"
            | "x-real-ip"
            | "true-client-ip"
            | "cf-connecting-ip"
            | "cf-ray"
            | "traceparent"
            | "tracestate"
            | "baggage"
            | "x-request-id"
            | "internal"
            | "x-internal"
            | "1flowbase"
            | "x-1flowbase"
    ) && !normalized.starts_with("x-1flowbase-")
        && !normalized.starts_with("x-internal-")
        && !normalized.starts_with("internal-")
        && !normalized.starts_with("x-forwarded-")
        && !normalized.starts_with("x-envoy-")
        && !normalized.starts_with("x-amzn-")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn wp_d2c_current_abi_restores_all_responses_residual_locations_deterministically() {
        let envelope: ProtocolContextEnvelope = serde_json::from_value(json!({
            "source_protocol": "openai_responses",
            "query": {"preview": ["one", "two"]},
            "headers": {"x-client-name": ["fixture", "fixture-v2"]},
            "body": {"future_option": {"shape": "opaque"}}
        }))
        .unwrap();
        let (body, context) = restore_protocol_context(
            OpenAiWireProtocol::Responses,
            json!({"model": "gpt-5.4", "input": [], "stream": true}),
            Some(&envelope),
        )
        .unwrap();
        let mut url = Url::parse("https://api.openai.com/v1/responses").unwrap();
        append_protocol_query(&mut url, &context).unwrap();
        let mut headers = HeaderMap::new();
        append_protocol_headers(&mut headers, &context).unwrap();

        assert_eq!(
            body,
            json!({
                "model": "gpt-5.4",
                "input": [],
                "stream": true,
                "future_option": {"shape": "opaque"}
            })
        );
        assert_eq!(url.query(), Some("preview=one&preview=two"));
        assert_eq!(
            headers
                .get_all("x-client-name")
                .iter()
                .map(|value| value.to_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["fixture", "fixture-v2"]
        );
    }

    #[test]
    fn wp_d2c_current_abi_rejects_typed_body_collisions_for_chat_and_responses() {
        for (protocol, source_protocol, collision) in [
            (OpenAiWireProtocol::Chat, "openai_chat", "messages"),
            (
                OpenAiWireProtocol::Responses,
                "openai_responses",
                "reasoning",
            ),
        ] {
            let envelope = ProtocolContextEnvelope {
                source_protocol: source_protocol.to_string(),
                body: BTreeMap::from([(collision.to_string(), json!({"must": "reject"}))]),
                ..ProtocolContextEnvelope::default()
            };
            let error = restore_protocol_context(protocol, json!({}), Some(&envelope)).unwrap_err();

            assert!(error.to_string().contains("collides with typed field"));
        }
    }

    #[test]
    fn wp_d2c_current_abi_rejects_owned_query_and_header_collisions() {
        let context = RestoredProtocolContext {
            query: BTreeMap::from([("preview".to_string(), vec!["residual".to_string()])]),
            headers: BTreeMap::from([(
                "openai-project".to_string(),
                vec!["residual-project".to_string()],
            )]),
        };
        let mut url = Url::parse("https://api.openai.com/v1/responses?preview=typed").unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("openai-project", HeaderValue::from_static("typed-project"));

        assert_eq!(
            append_protocol_query(&mut url, &context)
                .unwrap_err()
                .to_string(),
            "protocol context query collides with an owned field: preview"
        );
        assert_eq!(
            append_protocol_headers(&mut headers, &context)
                .unwrap_err()
                .to_string(),
            "protocol context header collides with an owned field: openai-project"
        );
    }

    #[test]
    fn wp_d2c_current_abi_rejects_reserved_fields_and_foreign_context() {
        let foreign = ProtocolContextEnvelope {
            source_protocol: "anthropic_messages".to_string(),
            body: BTreeMap::from([("future_option".to_string(), json!(true))]),
            ..ProtocolContextEnvelope::default()
        };
        assert!(
            restore_protocol_context(OpenAiWireProtocol::Responses, json!({}), Some(&foreign))
                .unwrap_err()
                .to_string()
                .contains("unconsumed foreign protocol context")
        );

        for envelope in [
            ProtocolContextEnvelope {
                source_protocol: "openai_responses".to_string(),
                query: BTreeMap::from([("x-api-key".to_string(), vec!["secret".to_string()])]),
                ..ProtocolContextEnvelope::default()
            },
            ProtocolContextEnvelope {
                source_protocol: "openai_responses".to_string(),
                headers: BTreeMap::from([(
                    "connection".to_string(),
                    vec!["keep-alive".to_string()],
                )]),
                ..ProtocolContextEnvelope::default()
            },
            ProtocolContextEnvelope {
                source_protocol: "openai_responses".to_string(),
                body: BTreeMap::from([(
                    "future_option".to_string(),
                    json!({"authorization": "nested-secret"}),
                )]),
                ..ProtocolContextEnvelope::default()
            },
        ] {
            assert!(restore_protocol_context(
                OpenAiWireProtocol::Responses,
                json!({}),
                Some(&envelope)
            )
            .unwrap_err()
            .to_string()
            .contains("reserved protocol context field"));
        }
    }

    #[test]
    fn wp_d2c_current_abi_rejects_unknown_envelope_shell_fields() {
        let error = serde_json::from_value::<ProtocolContextEnvelope>(json!({
            "source_protocol": "openai_responses",
            "query": {},
            "headers": {},
            "body": {},
            "fallback": true
        }))
        .unwrap_err();

        assert!(error.to_string().contains("unknown field"));
    }
}
