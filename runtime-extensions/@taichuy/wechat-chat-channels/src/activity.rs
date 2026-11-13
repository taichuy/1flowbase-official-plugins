//! iLink native typing, matching Hermes' ticket lifetime and start/stop protocol.
use super::*;
const TICKET_TTL_MS: u64 = 600_000;

pub(super) async fn set(
    client: &impl ILink,
    secrets: &Secrets,
    target: NotificationSendTarget,
    state: NotificationActivityState,
    clock: &impl Fn() -> u64,
) -> Result<NotificationRuntimeResult, ProviderError> {
    let mut patch = Secrets::new();
    let result: Result<bool, ProviderError> = async {
        if !has_account(secrets) {
            return Ok(false);
        }
        let recipient = secret(secrets, "bound_user_id").ok_or(ProviderError::Malformed)?;
        if target.sender_id != recipient || target.conversation_id != recipient {
            return Ok(false);
        }
        let cached = secret(secrets, "typing_ticket").filter(|_| {
            secrets
                .get("typing_ticket_expires_at_ms")
                .and_then(Value::as_u64)
                .is_some_and(|expiry| expiry > clock())
        });
        let ticket = if let Some(ticket) = cached {
            ticket.to_owned()
        } else {
            let mut body = json!({"ilink_user_id": recipient});
            if let Some(context) = secret(secrets, "context_token") {
                body["context_token"] = json!(context);
            }
            let config = client
                .request(request(
                    secrets,
                    "ilink/bot/getconfig",
                    body,
                    Duration::from_secs(3),
                )?)
                .await?;
            let Some(ticket) = config["typing_ticket"]
                .as_str()
                .filter(|ticket| !ticket.is_empty())
            else {
                return Ok(false);
            };
            if rejected(&config) {
                return Ok(false);
            }
            patch.insert("typing_ticket".into(), json!(ticket));
            patch.insert(
                "typing_ticket_expires_at_ms".into(),
                json!(clock().saturating_add(TICKET_TTL_MS)),
            );
            ticket.to_owned()
        };
        // Stop also refreshes an expired ticket, otherwise the indicator may remain visible.
        let body = json!({"ilink_user_id": recipient, "typing_ticket": ticket,
            "status": if state == NotificationActivityState::Replying { 1 } else { 2 }});
        let response = client
            .request(request(
                secrets,
                "ilink/bot/sendtyping",
                body,
                Duration::from_secs(3),
            )?)
            .await?;
        if rejected(&response) {
            patch.extend(clear(&["typing_ticket", "typing_ticket_expires_at_ms"]));
            return Ok(false);
        }
        Ok(true)
    }
    .await;
    // Native status is optional; it must never turn a report into an error reply.
    Ok(NotificationRuntimeResult::Activity {
        applied: result.unwrap_or(false),
        managed_secret_patch: patch,
    })
}

fn rejected(response: &Value) -> bool {
    !response.is_object()
        || ["ret", "errcode"].iter().any(|key| {
            response
                .get(key)
                .is_some_and(|value| value.as_i64() != Some(0))
        })
}
