// Expectations: provider protocol and Root's approved P3 packet, not local state shortcuts.
use super::*;
use std::{cell::RefCell, collections::VecDeque};

struct Mock {
    responses: RefCell<VecDeque<Result<Value, ProviderError>>>,
    requests: RefCell<Vec<Request>>,
}
impl Mock {
    fn new(responses: Vec<Result<Value, ProviderError>>) -> Self {
        Self {
            responses: RefCell::new(responses.into()),
            requests: RefCell::new(vec![]),
        }
    }
}
impl ILink for Mock {
    fn request(&self, request: Request) -> ProviderFuture<'_> {
        self.requests.borrow_mut().push(request);
        let response = self
            .responses
            .borrow_mut()
            .pop_front()
            .expect("unexpected supplier I/O");
        Box::pin(async move { response })
    }
}
fn secrets() -> Secrets {
    serde_json::from_value(json!({"bot_token":"private-token", "bot_id":"bot", "base_url":DEFAULT_BASE, "bound_user_id":"scanner"})).unwrap()
}
fn input(operation: NotificationOperation, managed_secrets: Secrets) -> NotificationRuntimeInput {
    NotificationRuntimeInput {
        operation,
        managed_secrets,
        delivery_window: None,
    }
}
fn auth(
    operation: NotificationAuthorizationOperation,
    secrets: Secrets,
) -> NotificationRuntimeInput {
    input(NotificationOperation::Authorize { operation }, secrets)
}
fn send(secrets: Secrets) -> NotificationRuntimeInput {
    let mut input = input(
        NotificationOperation::Send {
            input_payload: json!({"text":"你好"}),
        },
        secrets,
    );
    let last = now_ms() - 1000;
    input.delivery_window = Some(subscription::cached(
        Some(NotificationDeliveryWindowView {
            status: NotificationDeliveryWindowStatus::Active,
            last_received_at_ms: Some(last),
            expires_at_ms: Some(last + subscription::WINDOW_MS),
        }),
        now_ms(),
    ));
    input
}

#[path = "subscription.rs"]
mod window_tests;
fn apply(secrets: &mut Secrets, result: &NotificationRuntimeResult) {
    let patch = match result {
        NotificationRuntimeResult::Authorization {
            managed_secret_patch,
            ..
        }
        | NotificationRuntimeResult::Send {
            managed_secret_patch,
            ..
        } => managed_secret_patch,
    };
    for (key, value) in patch {
        if value.is_null() {
            secrets.remove(key);
        } else {
            secrets.insert(key.clone(), value.clone());
        }
    }
}
fn view(result: &NotificationRuntimeResult) -> NotificationAuthorizationStatus {
    match result {
        NotificationRuntimeResult::Authorization { authorization, .. } => authorization.status,
        NotificationRuntimeResult::Send { authorization, .. } => {
            authorization.as_ref().unwrap().status
        }
    }
}
fn failure(result: &NotificationRuntimeResult, expected: NotificationFailureCode) {
    assert!(
        matches!(result, NotificationRuntimeResult::Send { outcome: NotificationSendOutcome::Failed { failure_code }, .. } if *failure_code == expected)
    );
}

#[test]
fn manifest_has_empty_config_and_private_secret_contract() {
    let manifest: Value = serde_yaml::from_str(include_str!("../../manifest.yaml")).unwrap();
    assert_eq!(
        manifest["version"],
        manifest["managed"]["module"]["module_version"]
    );
    let descriptor = descriptor();
    descriptor
        .validate_plugin_namespace("1flowbase", "wechat-notification")
        .unwrap();
    assert!(descriptor.validate_config(&json!({})).is_ok());
    assert!(descriptor
        .validate_config(&json!({"base_url":"http://localhost"}))
        .is_err());
    assert!(descriptor.validate_input(&json!({"text":""})).is_err());
    assert!(descriptor
        .validate_output(&json!({"accepted":false,"delivery_id":"x"}))
        .is_err());
}

#[test]
fn notification_presentation_dictionaries_belong_to_the_plugin_and_preserve_wire_keys() {
    let descriptor = descriptor();
    descriptor.validate().unwrap();
    let zh = &descriptor.i18n["zh_Hans"];
    let en = &descriptor.i18n["en_US"];
    assert_eq!(zh.display_name.as_deref(), Some("微信私信通知"));
    assert_eq!(en.display_name.as_deref(), Some("WeChat private text"));
    assert_eq!(
        zh.input_form.as_ref().unwrap().fields["text"]
            .label
            .as_deref(),
        Some("消息正文")
    );
    assert_eq!(
        zh.authorization.as_ref().unwrap().actions["qr"].label,
        "微信扫码授权"
    );
    assert_eq!(descriptor.input_form.fields[0].key, "text");
    assert_eq!(
        descriptor.authorization.as_ref().unwrap().actions[0].code,
        "qr"
    );
}

#[tokio::test]
async fn qr_flow_recovers_host_secrets_redirects_then_binds_scanner() {
    let mock = Mock::new(vec![
        Ok(json!({"qrcode":"challenge", "qrcode_img_content":"https://weixin.qq.com/qr"})),
        Ok(json!({"status":"wait"})),
        Ok(json!({"status":"scaned"})),
        Ok(json!({"status":"scaned_but_redirect","redirect_host":"ilinkai2.weixin.qq.com"})),
        Ok(
            json!({"status":"confirmed","bot_token":"token", "ilink_bot_id":"bot", "ilink_user_id":"scanner", "baseurl":"https://ilinkai2.weixin.qq.com"}),
        ),
    ]);
    let mut stored = Secrets::new();
    let begun = run(
        &mock,
        auth(
            NotificationAuthorizationOperation::Begin {
                action: "qr".into(),
            },
            stored.clone(),
        ),
    )
    .await
    .unwrap();
    assert_eq!(view(&begun), NotificationAuthorizationStatus::Pending);
    apply(&mut stored, &begun);
    assert_eq!(stored["qr_credential"], "challenge");
    for _ in 0..3 {
        let polled = run(
            &mock,
            auth(NotificationAuthorizationOperation::Poll, stored.clone()),
        )
        .await
        .unwrap();
        assert_eq!(view(&polled), NotificationAuthorizationStatus::Pending);
        apply(&mut stored, &polled);
    }
    let confirmed = run(
        &mock,
        auth(NotificationAuthorizationOperation::Poll, stored.clone()),
    )
    .await
    .unwrap();
    assert_eq!(
        view(&confirmed),
        NotificationAuthorizationStatus::Authorized
    );
    apply(&mut stored, &confirmed);
    assert_eq!(stored["bound_user_id"], "scanner");
    assert_eq!(stored["bot_token"], "token");
    assert!(!stored.contains_key("qr_credential"));
    let requests = mock.requests.borrow();
    assert_eq!(requests[0].query, vec![("bot_type".into(), "3".into())]);
    assert_eq!(requests[4].base_url, "https://ilinkai2.weixin.qq.com");
}

#[tokio::test]
async fn direct_send_without_context_is_attempted_and_only_explicit_success_succeeds() {
    for poll in [Ok(json!({"ret":0,"msgs":[]})), Err(ProviderError::Timeout)] {
        let mock = Mock::new(vec![poll, Ok(json!({"ret":0}))]);
        let result = run(&mock, send(secrets())).await.unwrap();
        assert!(matches!(
            result,
            NotificationRuntimeResult::Send {
                outcome: NotificationSendOutcome::Succeeded { .. },
                ..
            }
        ));
        let requests = mock.requests.borrow();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].timeout, Duration::from_secs(1));
        let msg = &requests[1].body.as_ref().unwrap()["msg"];
        assert_eq!(msg["to_user_id"], "scanner");
        assert!(msg.get("context_token").is_none());
        assert!(!msg["client_id"].as_str().unwrap().is_empty());
        assert_eq!(msg["item_list"][0]["text_item"]["text"], "你好");
    }
}

#[tokio::test]
async fn message_id_acknowledges_send_without_default_status_fields() {
    // Real iLink sendmessage returned HTTP 200 with only message_id on 2026-10-15;
    // the scanner independently confirmed receipt. Default zero status fields may be absent.
    for response in [
        json!({"message_id":"provider-message-123"}),
        json!({"message_id":123}),
    ] {
        let mock = Mock::new(vec![Ok(json!({"ret":0,"msgs":[]})), Ok(response)]);
        let result = run(&mock, send(secrets())).await.unwrap();
        assert!(matches!(
            result,
            NotificationRuntimeResult::Send {
                outcome: NotificationSendOutcome::Succeeded { .. },
                ..
            }
        ));
        assert_eq!(mock.requests.borrow().len(), 2);
    }
}

#[tokio::test]
async fn message_id_cannot_override_provider_errors_or_malformed_status() {
    for response in [
        json!({"message_id":"ack","ret":-2}),
        json!({"message_id":"ack","errcode":-14}),
    ] {
        let expected = if response.get("ret").is_some() {
            NotificationFailureCode::RecipientNotReady
        } else {
            NotificationFailureCode::AuthorizationInvalid
        };
        let mock = Mock::new(vec![Ok(json!({"ret":0,"msgs":[]})), Ok(response)]);
        failure(&run(&mock, send(secrets())).await.unwrap(), expected);
    }
    for response in [
        json!({"message_id":"ack","ret":"0"}),
        json!({"message_id":"ack","errcode":null}),
        json!({"message_id":""}),
        json!({"message_id":null}),
        json!({"message_id":0}),
        json!({"message_id":true}),
        json!({}),
    ] {
        let mock = Mock::new(vec![Ok(json!({"ret":0,"msgs":[]})), Ok(response)]);
        assert!(matches!(
            run(&mock, send(secrets())).await.unwrap(),
            NotificationRuntimeResult::Send {
                outcome: NotificationSendOutcome::Unknown { .. },
                ..
            }
        ));
        assert_eq!(mock.requests.borrow().len(), 2);
    }
}

#[tokio::test]
async fn inbound_context_matches_only_bound_scanner_and_never_rebinds() {
    let mock = Mock::new(vec![
        Ok(json!({"ret":0,"get_updates_buf":"cursor2","msgs":[
            {"from_user_id":"scanner","message_type":1,"create_time_ms":now_ms(),"context_token":"correct"},
            {"from_user_id":"attacker","context_token":"wrong"}
        ]})),
        Ok(json!({"ret":0})),
    ]);
    let mut stored = secrets();
    let result = run(&mock, send(stored.clone())).await.unwrap();
    apply(&mut stored, &result);
    assert_eq!(stored["bound_user_id"], "scanner");
    assert_eq!(stored["context_token"], "correct");
    assert_eq!(stored["cursor"], "cursor2");
    assert_eq!(
        mock.requests.borrow()[1].body.as_ref().unwrap()["msg"]["context_token"],
        "correct"
    );
    let other = Mock::new(vec![Ok(
        json!({"ret":0,"msgs":[{"from_user_id":"attacker","context_token":"wrong"}]}),
    )]);
    let maintained = run(
        &other,
        auth(NotificationAuthorizationOperation::Maintain, stored.clone()),
    )
    .await
    .unwrap();
    apply(&mut stored, &maintained);
    assert_eq!(stored["context_token"], "correct");
}

#[tokio::test]
async fn definitive_failures_are_classified_without_revoking_recipient_not_ready() {
    for (response, expected, status) in [
        (
            json!({"ret":-14}),
            NotificationFailureCode::AuthorizationInvalid,
            NotificationAuthorizationStatus::Invalid,
        ),
        (
            json!({"ret":-2,"errmsg":"prepare failed"}),
            NotificationFailureCode::RecipientNotReady,
            NotificationAuthorizationStatus::Authorized,
        ),
        (
            json!({"ret":9}),
            NotificationFailureCode::ProviderRejected,
            NotificationAuthorizationStatus::Authorized,
        ),
    ] {
        let mock = Mock::new(vec![Ok(json!({"ret":0,"msgs":[]})), Ok(response)]);
        let mut stored = secrets();
        stored.insert("context_token".into(), json!("context"));
        let result = run(&mock, send(stored.clone())).await.unwrap();
        failure(&result, expected);
        assert_eq!(view(&result), status);
        apply(&mut stored, &result);
        assert_eq!(
            mock.requests.borrow().len(),
            2,
            "one preparation and one send, never retry send blindly"
        );
        if status == NotificationAuthorizationStatus::Invalid {
            assert!(!stored.contains_key("bot_token"));
            let restored = run(
                &Mock::new(vec![]),
                auth(NotificationAuthorizationOperation::Maintain, stored),
            )
            .await
            .unwrap();
            assert_eq!(view(&restored), status);
        } else {
            assert!(stored.contains_key("bot_token"));
        }
    }
}

#[tokio::test]
async fn send_loss_and_malformed_response_are_unknown_and_never_retried() {
    for response in [
        Err(ProviderError::Network),
        Err(ProviderError::Malformed),
        Ok(json!({})),
        Ok(json!({"ret":"0"})),
    ] {
        let mock = Mock::new(vec![Ok(json!({"ret":0,"msgs":[]})), response]);
        let mut stored = secrets();
        stored.insert("context_token".into(), json!("context"));
        let result = run(&mock, send(stored)).await.unwrap();
        assert!(matches!(
            result,
            NotificationRuntimeResult::Send {
                outcome: NotificationSendOutcome::Unknown {
                    failure_code: NotificationFailureCode::ProviderResultUnknown
                },
                ..
            }
        ));
        assert_eq!(mock.requests.borrow().len(), 2);
    }
}

#[tokio::test]
async fn missing_credentials_and_expired_qr_do_not_send() {
    let mock = Mock::new(vec![]);
    let result = run(&mock, send(Secrets::new())).await.unwrap();
    failure(&result, NotificationFailureCode::AuthorizationRequired);
    assert_eq!(view(&result), NotificationAuthorizationStatus::Unauthorized);
    let mut stored = Secrets::new();
    stored.insert("qr_credential".into(), json!("challenge"));
    stored.insert("qr_code_url".into(), json!("https://weixin.qq.com/qr"));
    let expired = Mock::new(vec![Ok(json!({"status":"expired"}))]);
    let result = run(
        &expired,
        auth(NotificationAuthorizationOperation::Poll, stored.clone()),
    )
    .await
    .unwrap();
    assert_eq!(view(&result), NotificationAuthorizationStatus::Unauthorized);
    apply(&mut stored, &result);
    assert!(!stored.contains_key("qr_credential"));
    let cancelled = run(
        &mock,
        auth(NotificationAuthorizationOperation::Cancel, secrets()),
    )
    .await
    .unwrap();
    assert_eq!(
        view(&cancelled),
        NotificationAuthorizationStatus::Authorized
    );
}

#[test]
fn provider_urls_reject_untrusted_hosts_credentials_ports_and_paths() {
    for url in [
        "http://ilinkai.weixin.qq.com",
        "https://ilinkai.weixin.qq.com.evil.com",
        "https://evilweixin.qq.com",
        "https://localhost",
        "https://user@ilinkai.weixin.qq.com",
        "https://ilinkai.weixin.qq.com:444",
        "https://ilinkai.weixin.qq.com/custom",
        "https://ilinkai.weixin.qq.com?secret=x",
    ] {
        assert!(trusted_base(url).is_err(), "{url}");
    }
    assert!(trusted_base(DEFAULT_BASE).is_ok());
}

#[tokio::test]
async fn preparation_failure_keeps_cached_context_but_invalid_token_stops_send() {
    let mut stored = secrets();
    stored.insert("context_token".into(), json!("old"));
    let mock = Mock::new(vec![Err(ProviderError::Network), Ok(json!({"ret":0}))]);
    let result = run(&mock, send(stored.clone())).await.unwrap();
    assert!(matches!(
        result,
        NotificationRuntimeResult::Send {
            outcome: NotificationSendOutcome::Succeeded { .. },
            ..
        }
    ));
    assert_eq!(
        mock.requests.borrow()[1].body.as_ref().unwrap()["msg"]["context_token"],
        "old"
    );
    let mock = Mock::new(vec![Ok(json!({"ret":0,"errcode":-14}))]);
    let result = run(&mock, send(stored.clone())).await.unwrap();
    failure(&result, NotificationFailureCode::AuthorizationInvalid);
    assert_eq!(mock.requests.borrow().len(), 1);
    apply(&mut stored, &result);
    assert!(!stored.contains_key("bot_token"));
}

#[tokio::test]
async fn cached_context_is_refreshed_before_each_send() {
    let mut stored = secrets();
    stored.insert("context_token".into(), json!("old"));
    let mock = Mock::new(vec![
        Ok(
            json!({"ret":0,"msgs":[{"from_user_id":"scanner","message_type":1,"create_time_ms":now_ms(),"context_token":"new"}]}),
        ),
        Ok(json!({"ret":0})),
    ]);
    let result = run(&mock, send(stored.clone())).await.unwrap();
    apply(&mut stored, &result);
    assert_eq!(stored["context_token"], "new");
    assert_eq!(
        mock.requests.borrow()[1].body.as_ref().unwrap()["msg"]["context_token"],
        "new"
    );
}

#[tokio::test]
async fn sdk_rejects_user_credentials_and_redacts_authorization_provider_errors() {
    let mock = Mock::new(vec![]);
    let request: CapabilityStdioRequest =
        serde_json::from_value(json!({"method":"execute","input":{
            "config_payload":{"bot_token":"user-supplied"}, "input_payload":send(secrets())
        }}))
        .unwrap();
    let result =
        execute_authorizable_notification(request, &descriptor(), |_, input| run(&mock, input))
            .await;
    assert!(!result.ok);
    assert!(mock.requests.borrow().is_empty());
    let mock = Mock::new(vec![Ok(
        json!({"qrcode":"PRIVATE-QR","qrcode_img_content":"bad-url", "bot_token":"PRIVATE-TOKEN"}),
    )]);
    let request: CapabilityStdioRequest = serde_json::from_value(json!({"method":"execute","input":{
        "config_payload":{}, "input_payload":auth(NotificationAuthorizationOperation::Begin {action:"qr".into()},Secrets::new())
    }})).unwrap();
    let result =
        execute_authorizable_notification(request, &descriptor(), |_, input| run(&mock, input))
            .await;
    assert!(!result.ok);
    let public = serde_json::to_string(&result).unwrap();
    assert!(!public.contains("PRIVATE"));
}

#[tokio::test]
async fn malicious_qr_redirect_and_incomplete_confirmation_do_not_authorize() {
    let mut stored = Secrets::new();
    stored.insert("qr_credential".into(), json!("challenge"));
    stored.insert("qr_code_url".into(), json!("https://weixin.qq.com/qr"));
    for response in [
        json!({"status":"redirect","redirect_host":"127.0.0.1"}),
        json!({"status":"confirmed","bot_token":"token","ilink_bot_id":"bot"}),
        json!({"status":"confirmed","bot_token":"token","ilink_bot_id":"bot","ilink_user_id":"scanner","baseurl":"https://evil.com"}),
    ] {
        let mock = Mock::new(vec![Ok(response)]);
        assert!(run(
            &mock,
            auth(NotificationAuthorizationOperation::Poll, stored.clone())
        )
        .await
        .is_err());
    }
    let cancelled = run(
        &Mock::new(vec![]),
        auth(NotificationAuthorizationOperation::Cancel, stored.clone()),
    )
    .await
    .unwrap();
    apply(&mut stored, &cancelled);
    assert_eq!(
        view(&cancelled),
        NotificationAuthorizationStatus::Unauthorized
    );
    assert!(stored.is_empty());
}

#[tokio::test]
async fn all_delivery_outcomes_and_secret_patches_obey_sdk_contract() {
    for response in [
        json!({"ret":0}),
        json!({"errcode":-14}),
        json!({"ret":-2}),
        json!({"ret":42}),
        json!({}),
    ] {
        let mock = Mock::new(vec![Ok(json!({"ret":0,"msgs":[]})), Ok(response)]);
        let input = send(secrets());
        let result = run(&mock, input.clone()).await.unwrap();
        result.validate_for(&input, &descriptor()).unwrap();
    }
}
