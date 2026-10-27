// Expectations: provider protocol and Root's approved P5 communications packet.
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
            target: None,
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

fn apply(secrets: &mut Secrets, result: &NotificationRuntimeResult) {
    let patch = match result {
        NotificationRuntimeResult::Authorization {
            managed_secret_patch,
            ..
        }
        | NotificationRuntimeResult::Send {
            managed_secret_patch,
            ..
        }
        | NotificationRuntimeResult::Receive {
            managed_secret_patch,
            ..
        } => managed_secret_patch,
        NotificationRuntimeResult::DownloadAttachment { .. } => return,
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
        NotificationRuntimeResult::Send { authorization, .. }
        | NotificationRuntimeResult::Receive { authorization, .. } => {
            authorization.as_ref().unwrap().status
        }
        NotificationRuntimeResult::DownloadAttachment { .. } => {
            panic!("no authorization in download result")
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
        .validate_plugin_namespace("1flowbase", "wechat-chat-channels")
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
    assert_eq!(zh.display_name.as_deref(), Some("微信私聊通讯"));
    assert_eq!(
        en.display_name.as_deref(),
        Some("WeChat private communications")
    );
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
async fn sdk_rejects_user_credentials_and_redacts_authorization_provider_errors() {
    let mock = Mock::new(vec![]);
    let request: CapabilityStdioRequest =
        serde_json::from_value(json!({"method":"execute","input":{
            "config_payload":{"bot_token":"user-supplied"}, "input_payload":send(secrets())
        }}))
        .unwrap();
    let result = execute_communication(request, &descriptor(), |_, input| run(&mock, input)).await;
    assert!(!result.ok);
    assert!(mock.requests.borrow().is_empty());
    let mock = Mock::new(vec![Ok(
        json!({"qrcode":"PRIVATE-QR","qrcode_img_content":"bad-url", "bot_token":"PRIVATE-TOKEN"}),
    )]);
    let request: CapabilityStdioRequest = serde_json::from_value(json!({"method":"execute","input":{
        "config_payload":{}, "input_payload":auth(NotificationAuthorizationOperation::Begin {action:"qr".into()},Secrets::new())
    }})).unwrap();
    let result = execute_communication(request, &descriptor(), |_, input| run(&mock, input)).await;
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

#[path = "communications.rs"]
mod communications;

#[test]
fn send_log_mapping_preserves_text_and_only_exposes_business_receipt() {
    let descriptor = descriptor();
    descriptor.validate().unwrap();
    let mapping = descriptor
        .log
        .as_ref()
        .expect("plugin declares send log mapping");
    let text = "微信😀".repeat(180);
    let snapshot = mapping.project_input(&json!({"text":text})).unwrap();
    assert_eq!(snapshot.content, text);
    assert_eq!(snapshot.title.chars().count(), 255);
    assert_eq!(snapshot.extra, json!({}));
    assert_eq!(
        mapping.project_result(
            &json!({"accepted":true,"delivery_id":"delivery-1","bot_token":"not-a-public-field"})
        ),
        json!({"accepted":true,"delivery_id":"delivery-1"})
    );
}
