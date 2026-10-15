//! WeChat's rolling reply window. Public evidence is separate from credentials.
use super::*;

pub(super) const WINDOW_MS: u64 = 24 * 60 * 60 * 1000;
type Window = NotificationDeliveryWindowView;
type WindowStatus = NotificationDeliveryWindowStatus;

pub(super) struct Refreshed {
    pub patch: Secrets,
    pub window: Window,
}

pub(super) fn waiting() -> Window {
    Window {
        status: WindowStatus::WaitingForMessage,
        last_received_at_ms: None,
        expires_at_ms: None,
    }
}
pub(super) fn unknown(previous: Option<Window>) -> Window {
    let mut window = previous.unwrap_or(Window {
        status: WindowStatus::Unknown,
        last_received_at_ms: None,
        expires_at_ms: None,
    });
    window.status = WindowStatus::Unknown;
    window
}
fn from_last(last: u64, now: u64) -> Option<Window> {
    if last == 0 || last > now {
        return None;
    }
    let expires = last.checked_add(WINDOW_MS)?;
    Some(Window {
        status: if now < expires {
            WindowStatus::Active
        } else {
            WindowStatus::Expired
        },
        last_received_at_ms: Some(last),
        expires_at_ms: Some(expires),
    })
}
pub(super) fn cached(window: Option<Window>, now: u64) -> Window {
    window
        .as_ref()
        .and_then(|window| window.last_received_at_ms)
        .and_then(|last| from_last(last, now))
        .unwrap_or_else(|| {
            if window
                .as_ref()
                .is_some_and(|window| window.status == WindowStatus::WaitingForMessage)
            {
                waiting()
            } else {
                unknown(None)
            }
        })
}

// getupdates omits default zero status fields in real first-party responses.
// Unlike sendmessage, it must have a valid update batch/cursor or explicit success.
fn updates_code(value: &Value) -> ProviderCode {
    let Some(object) = value.as_object() else {
        return ProviderCode::Unknown;
    };
    for key in ["ret", "errcode"] {
        if object
            .get(key)
            .is_some_and(|value| value.as_i64().is_none())
        {
            return ProviderCode::Unknown;
        }
    }
    if matches!(provider_code(value), ProviderCode::Failure(_)) {
        return provider_code(value);
    }
    if object.get("msgs").is_some_and(|value| !value.is_array())
        || object
            .get("get_updates_buf")
            .is_some_and(|value| !value.is_string())
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

pub(super) async fn refresh(
    client: &impl ILink,
    secrets: &Secrets,
    previous: Option<Window>,
    clock: &impl Fn() -> u64,
) -> Result<Refreshed, ProviderError> {
    let req = request(
        secrets,
        "ilink/bot/getupdates",
        json!({"get_updates_buf":secret(secrets,"cursor").unwrap_or("")}),
        Duration::from_secs(1),
    )?;
    // Cancellation of a long poll is unavailable evidence, never an empty message batch.
    let response = client.request(req).await?;
    let now = clock();
    match updates_code(&response) {
        ProviderCode::Failure(NotificationFailureCode::AuthorizationInvalid) => {
            return Ok(Refreshed {
                patch: invalid_patch(),
                window: unknown(None),
            });
        }
        ProviderCode::Success => {}
        _ => return Err(ProviderError::Malformed),
    }
    let mut patch = Secrets::new();
    if let Some(cursor) = response["get_updates_buf"].as_str() {
        patch.insert("cursor".into(), json!(cursor));
    }
    let mut window = cached(previous, now);
    let mut newest = window.last_received_at_ms.unwrap_or(0);
    let mut unverified_time = false;
    if let Some(messages) = response["msgs"].as_array() {
        for message in messages {
            if message["from_user_id"].as_str() != secret(secrets, "bound_user_id")
                || message["message_type"].as_u64() != Some(1)
            {
                continue;
            }
            let Some(last) = message["create_time_ms"]
                .as_u64()
                .filter(|last| from_last(*last, now).is_some())
            else {
                unverified_time = true;
                continue;
            };
            if last < newest {
                continue;
            }
            let is_newer = last > newest;
            newest = last;
            window = from_last(last, now).expect("validated provider timestamp");
            // A valid message renews the window even without text or context_token.
            if let Some(context) = message["context_token"]
                .as_str()
                .filter(|value| !value.is_empty())
            {
                patch.insert("context_token".into(), json!(context));
            } else if is_newer {
                patch.insert("context_token".into(), Value::Null);
            }
        }
    }
    if unverified_time && window.status != WindowStatus::Active {
        window = unknown(Some(window));
    }
    Ok(Refreshed { patch, window })
}

pub(super) async fn prepare(
    client: &impl ILink,
    secrets: &Secrets,
    previous: Option<Window>,
    clock: &impl Fn() -> u64,
) -> Refreshed {
    match refresh(client, secrets, previous.clone(), clock).await {
        Ok(result) => result,
        Err(_) => {
            let cached = cached(previous, clock());
            Refreshed {
                patch: Secrets::new(),
                window: if cached.status == WindowStatus::Active {
                    cached
                } else {
                    unknown(Some(cached))
                },
            }
        }
    }
}

pub(super) fn failure(window: &Window) -> Option<NotificationFailureCode> {
    match window.status {
        WindowStatus::Active => None,
        WindowStatus::WaitingForMessage => Some(NotificationFailureCode::SubscriptionRequired),
        WindowStatus::Expired => Some(NotificationFailureCode::SubscriptionExpired),
        WindowStatus::Unknown => Some(NotificationFailureCode::SubscriptionUnavailable),
    }
}
