//! The only getupdates consumer. Cursor candidates become committed only in Host's inbox transaction.
use super::*;
use runtime_extension_sdk::{
    NotificationReceiveBatch, NotificationReceiveOutcome, NotificationReceivedMessage,
    NotificationSender,
};
use std::collections::BTreeSet;

fn updates_code(value: &Value) -> ProviderCode {
    let Some(object) = value.as_object() else {
        return ProviderCode::Unknown;
    };
    if ["ret", "errcode"]
        .iter()
        .any(|key| object.get(*key).is_some_and(|v| v.as_i64().is_none()))
    {
        return ProviderCode::Unknown;
    }
    if matches!(provider_code(value), ProviderCode::Failure(_)) {
        return provider_code(value);
    }
    if object.get("msgs").is_some_and(|v| !v.is_array())
        || object
            .get("get_updates_buf")
            .is_some_and(|v| !v.is_string())
    {
        return ProviderCode::Unknown;
    }
    if value["ret"] == 0
        || value["errcode"] == 0
        || object.contains_key("msgs")
        || object.contains_key("get_updates_buf")
    {
        ProviderCode::Success
    } else {
        ProviderCode::Unknown
    }
}
fn failed_receive(
    code: NotificationFailureCode,
    secrets: &Secrets,
    patch: Secrets,
) -> NotificationRuntimeResult {
    NotificationRuntimeResult::Receive {
        outcome: NotificationReceiveOutcome::Failed { failure_code: code },
        authorization: Some(plain_view(
            if code == NotificationFailureCode::AuthorizationInvalid {
                NotificationAuthorizationStatus::Invalid
            } else {
                status(secrets)
            },
        )),
        managed_secret_patch: patch,
    }
}
fn message_id(message: &Value) -> Option<String> {
    match &message["message_id"] {
        Value::String(id) if !id.trim().is_empty() => Some(id.clone()),
        Value::Number(id) if id.as_u64().is_some_and(|id| id > 0) => Some(id.to_string()),
        _ => None,
    }
}
#[allow(clippy::too_many_arguments)]
pub(super) async fn receive(
    client: &impl ILink,
    secrets: &Secrets,
    previous: Option<NotificationDeliveryWindowView>,
    cursor: Option<Value>,
    max_messages: u32,
    wait_seconds: u32,
    clock: &impl Fn() -> u64,
) -> Result<NotificationRuntimeResult, ProviderError> {
    if !has_account(secrets) {
        return Ok(failed_receive(
            if status(secrets) == NotificationAuthorizationStatus::Invalid {
                NotificationFailureCode::AuthorizationInvalid
            } else {
                NotificationFailureCode::AuthorizationRequired
            },
            secrets,
            Secrets::new(),
        ));
    }
    if max_messages == 0 || max_messages > 100 || wait_seconds > 30 {
        return Err(ProviderError::Malformed);
    }
    // The opaque cursor can retain an unconsumed provider batch. Never truncate a batch
    // and advance the remote cursor past messages which Host has not persisted.
    let cursor = cursor.unwrap_or(Value::Null);
    let (mut raw, remote_cursor) =
        if let Some(pending) = cursor.get("pending").and_then(Value::as_array) {
            (
                pending.clone(),
                cursor
                    .get("provider_cursor")
                    .cloned()
                    .unwrap_or(Value::Null),
            )
        } else {
            let remote = if cursor.is_string() {
                cursor.clone()
            } else {
                cursor.get("provider_cursor").cloned().unwrap_or(json!(""))
            };
            let response = client
                .request(request(
                    secrets,
                    "ilink/bot/getupdates",
                    json!({"get_updates_buf":remote,"longpolling_timeout_ms":wait_seconds * 1000}),
                    Duration::from_secs(u64::from(wait_seconds) + 5),
                )?)
                .await?;
            match updates_code(&response) {
                ProviderCode::Failure(code) => {
                    return Ok(failed_receive(
                        code,
                        secrets,
                        if code == NotificationFailureCode::AuthorizationInvalid {
                            invalid_patch()
                        } else {
                            Secrets::new()
                        },
                    ))
                }
                ProviderCode::Success => {}
                _ => return Err(ProviderError::Malformed),
            }
            (
                response["msgs"].as_array().cloned().unwrap_or_default(),
                response.get("get_updates_buf").cloned().unwrap_or(remote),
            )
        };
    let now = clock();
    let mut window = subscription::cached(previous, now);
    let mut patch = Secrets::new();
    let mut messages = Vec::new();
    let mut seen = BTreeSet::new();
    let mut consumed = 0;
    for message in &raw {
        if messages.len() == max_messages as usize {
            break;
        }
        consumed += 1;
        if message["from_user_id"].as_str() != secret(secrets, "bound_user_id")
            || message["message_type"].as_u64() != Some(1)
        {
            continue;
        }
        // Invalid identified content must not advance the cursor and silently lose data.
        let id = message_id(message).ok_or(ProviderError::Malformed)?;
        if !seen.insert(id.clone()) {
            continue;
        }
        let received_at_ms = message["create_time_ms"]
            .as_u64()
            .filter(|v| *v > 0 && *v <= now)
            .ok_or(ProviderError::Malformed)?;
        let mut texts = Vec::new();
        let mut attachments = Vec::new();
        for (index, item) in message["item_list"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            if item["type"] == 1 {
                if let Some(text) = item["text_item"]["text"]
                    .as_str()
                    .filter(|v| !v.trim().is_empty())
                {
                    texts.push(text.to_string());
                }
            } else if let Some(attachment) = media::attachment(item, &id, index)? {
                attachments.push(attachment);
            }
        }
        if received_at_ms >= window.last_received_at_ms.unwrap_or(0) {
            window =
                subscription::from_last(received_at_ms, now).ok_or(ProviderError::Malformed)?;
            patch.insert(
                "context_token".into(),
                message
                    .get("context_token")
                    .filter(|v| v.as_str().is_some_and(|v| !v.is_empty()))
                    .cloned()
                    .unwrap_or(Value::Null),
            );
        }
        // Provider control/typing messages have no durable user content.
        if texts.is_empty() && attachments.is_empty() {
            continue;
        }
        messages.push(NotificationReceivedMessage {
            provider_message_id: id,
            conversation_id: secret(secrets, "bound_user_id")
                .expect("account checked")
                .into(),
            sender: NotificationSender {
                id: secret(secrets, "bound_user_id").unwrap().into(),
                display_name: None,
            },
            received_at_ms,
            text: if texts.is_empty() {
                None
            } else {
                Some(texts.join("\n"))
            },
            attachments,
        });
    }
    raw.drain(..consumed);
    let has_more = !raw.is_empty();
    let candidate_cursor = Some(if has_more {
        json!({"provider_cursor":remote_cursor,"pending":raw})
    } else {
        remote_cursor
    });
    let batch = NotificationReceiveBatch {
        messages,
        candidate_cursor,
        has_more,
    };
    batch
        .validate(max_messages)
        .map_err(|_| ProviderError::Malformed)?;
    Ok(with_window(
        NotificationRuntimeResult::Receive {
            outcome: NotificationReceiveOutcome::Succeeded { batch },
            authorization: Some(plain_view(NotificationAuthorizationStatus::Authorized)),
            managed_secret_patch: patch,
        },
        window,
    ))
}
