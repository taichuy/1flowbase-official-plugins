use super::*;
const NOW: u64 = 1_800_000_000_000;
const DAY: u64 = 86_400_000;
fn known(last: u64) -> NotificationDeliveryWindowView {
    NotificationDeliveryWindowView {
        status: NotificationDeliveryWindowStatus::Active,
        last_received_at_ms: Some(last),
        expires_at_ms: Some(last + DAY),
    }
}
fn request_with(window: Option<NotificationDeliveryWindowView>) -> NotificationRuntimeInput {
    let mut input = input(
        NotificationOperation::Send {
            input_payload: json!({"text":"window test"}),
        },
        secrets(),
    );
    input.delivery_window = window;
    input
}
fn result_window(result: &NotificationRuntimeResult) -> NotificationDeliveryWindowView {
    match result {
        NotificationRuntimeResult::Send { authorization, .. } => authorization
            .as_ref()
            .unwrap()
            .delivery_window
            .clone()
            .unwrap(),
        NotificationRuntimeResult::Authorization { authorization, .. } => {
            authorization.delivery_window.clone().unwrap()
        }
    }
}
async fn at(mock: &Mock, input: NotificationRuntimeInput, now: u64) -> NotificationRuntimeResult {
    let result = run_with_clock(mock, input.clone(), &|| now).await.unwrap();
    result.validate_for(&input, &descriptor()).unwrap();
    result
}
fn accepted(result: &NotificationRuntimeResult) {
    assert!(matches!(
        result,
        NotificationRuntimeResult::Send {
            outcome: NotificationSendOutcome::Succeeded { .. },
            ..
        }
    ));
}

#[tokio::test]
async fn waiting_expired_and_legacy_unknown_block_before_external_send() {
    for (window, code) in [
        (
            Some(subscription::waiting()),
            NotificationFailureCode::SubscriptionRequired,
        ),
        (
            Some(known(NOW - DAY)),
            NotificationFailureCode::SubscriptionExpired,
        ),
        (None, NotificationFailureCode::SubscriptionUnavailable),
    ] {
        let mock = Mock::new(vec![Ok(json!({"msgs":[],"get_updates_buf":"next"}))]);
        let result = at(&mock, request_with(window), NOW).await;
        failure(&result, code);
        assert_eq!(view(&result), NotificationAuthorizationStatus::Authorized);
        assert_eq!(mock.requests.borrow().len(), 1);
        let mut stored = secrets();
        apply(&mut stored, &result);
        assert_eq!(stored["bot_token"], "private-token");
        assert_eq!(stored["cursor"], "next");
    }
}

#[tokio::test]
async fn any_bound_user_message_renews_without_text_or_context_and_default_ret() {
    let last = NOW - 1234;
    let mock = Mock::new(vec![
        Ok(
            json!({"msgs":[{"from_user_id":"scanner","message_type":1,"create_time_ms":last,"item_list":[{"type":2}]}]}),
        ),
        Ok(json!({"message_id":"ack"})),
    ]);
    let result = at(&mock, request_with(None), NOW).await;
    accepted(&result);
    assert_eq!(result_window(&result), known(last));
    assert!(mock.requests.borrow()[1].body.as_ref().unwrap()["msg"]
        .get("context_token")
        .is_none());
}

#[tokio::test]
async fn newer_message_rolls_window_from_message_time_and_survives_next_request() {
    let old = known(NOW - 20 * 3_600_000);
    let last = NOW - 3_600_000;
    let mock = Mock::new(vec![
        Ok(json!({"get_updates_buf":"cursor-new","msgs":[
            {"from_user_id":"scanner","message_type":1,"create_time_ms":last,"context_token":"renewed"}
        ]})),
        Ok(json!({"ret":0})),
    ]);
    let result = at(&mock, request_with(Some(old)), NOW).await;
    accepted(&result);
    let window = result_window(&result);
    assert_eq!(window.last_received_at_ms, Some(last));
    assert_eq!(window.expires_at_ms, Some(last + DAY));
    let mut next = request_with(Some(window.clone()));
    apply(&mut next.managed_secrets, &result);
    let mock = Mock::new(vec![Ok(json!({"msgs":[]})), Ok(json!({"message_id":42}))]);
    let result = at(&mock, next, NOW + 10_000).await;
    accepted(&result);
    assert_eq!(result_window(&result), window);
    assert_eq!(
        mock.requests.borrow()[0].body.as_ref().unwrap()["get_updates_buf"],
        "cursor-new"
    );
}

#[tokio::test]
async fn reordered_duplicates_other_senders_and_bot_messages_never_extend_window() {
    let last = NOW - 1000;
    let mock = Mock::new(vec![
        Ok(json!({"msgs":[
            {"from_user_id":"scanner","message_type":1,"create_time_ms":last,"context_token":"current"},
            {"from_user_id":"scanner","message_type":1,"create_time_ms":last-2000,"context_token":"old"},
            {"from_user_id":"attacker","message_type":1,"create_time_ms":NOW,"context_token":"other"},
            {"from_user_id":"scanner","message_type":2,"create_time_ms":NOW,"context_token":"bot"},
            {"from_user_id":"scanner","message_type":1,"create_time_ms":NOW+1,"context_token":"future"}
        ]})),
        Ok(json!({"ret":0})),
    ]);
    let result = at(&mock, request_with(Some(known(last))), NOW).await;
    accepted(&result);
    assert_eq!(result_window(&result), known(last));
    let mut stored = secrets();
    apply(&mut stored, &result);
    assert_eq!(stored["bound_user_id"], "scanner");
    assert_eq!(stored["context_token"], "current");
}

#[tokio::test]
async fn missing_malformed_and_future_timestamps_never_use_poll_time() {
    for time in [
        Value::Null,
        json!(0),
        json!(NOW + 1),
        json!("1800000000000"),
        json!(true),
        json!(u64::MAX),
    ] {
        let mock = Mock::new(vec![Ok(
            json!({"msgs":[{"from_user_id":"scanner","message_type":1,"create_time_ms":time,"update_time_ms":NOW}]}),
        )]);
        let result = at(&mock, request_with(Some(subscription::waiting())), NOW).await;
        failure(&result, NotificationFailureCode::SubscriptionUnavailable);
        let window = result_window(&result);
        assert_eq!(window.status, NotificationDeliveryWindowStatus::Unknown);
        assert!(window.last_received_at_ms.is_none());
        assert_eq!(mock.requests.borrow().len(), 1);
    }
}

#[tokio::test]
async fn exact_expiry_boundary_blocks_but_one_millisecond_before_is_active() {
    let window = known(NOW - DAY);
    let mock = Mock::new(vec![Ok(json!({"msgs":[]})), Ok(json!({"ret":0}))]);
    accepted(&at(&mock, request_with(Some(window.clone())), NOW - 1).await);
    let mock = Mock::new(vec![Ok(json!({"msgs":[]}))]);
    let result = at(&mock, request_with(Some(window)), NOW).await;
    failure(&result, NotificationFailureCode::SubscriptionExpired);
    assert_eq!(mock.requests.borrow().len(), 1);
}

#[tokio::test]
async fn sync_errors_preserve_valid_evidence_but_do_not_claim_an_empty_batch() {
    for error in [
        ProviderError::Network,
        ProviderError::Timeout,
        ProviderError::Malformed,
    ] {
        let mock = Mock::new(vec![Err(error)]);
        let result = at(&mock, request_with(Some(known(NOW - DAY))), NOW).await;
        failure(&result, NotificationFailureCode::SubscriptionUnavailable);
        assert_eq!(result_window(&result).last_received_at_ms, Some(NOW - DAY));
        assert_eq!(mock.requests.borrow().len(), 1);
    }
    let mock = Mock::new(vec![
        Err(ProviderError::Timeout),
        Ok(json!({"message_id":"accepted"})),
    ]);
    accepted(&at(&mock, request_with(Some(known(NOW - 1000))), NOW).await);
}

#[tokio::test]
async fn malformed_update_response_is_not_success_and_does_not_send() {
    for response in [
        json!({}),
        json!({"ret":"0","msgs":[]}),
        json!({"msgs":{}}),
        json!({"errcode":null,"msgs":[]}),
        json!({"get_updates_buf":123}),
    ] {
        let mock = Mock::new(vec![Ok(response)]);
        let result = at(&mock, request_with(Some(subscription::waiting())), NOW).await;
        failure(&result, NotificationFailureCode::SubscriptionUnavailable);
        assert_eq!(mock.requests.borrow().len(), 1);
    }
}

#[tokio::test]
async fn maintenance_refreshes_public_window_without_sending_or_persisting_it_as_secret() {
    let mut input = auth(NotificationAuthorizationOperation::Maintain, secrets());
    input.delivery_window = Some(subscription::waiting());
    let mock = Mock::new(vec![Ok(
        json!({"msgs":[{"from_user_id":"scanner","message_type":1,"create_time_ms":NOW-100}]}),
    )]);
    let result = at(&mock, input, NOW).await;
    assert_eq!(result_window(&result), known(NOW - 100));
    assert_eq!(mock.requests.borrow().len(), 1);
    let mut stored = secrets();
    apply(&mut stored, &result);
    assert!(stored.keys().all(|key| !key.contains("window")
        && !key.contains("received_at")
        && !key.contains("expires")));
}

#[tokio::test]
async fn expiry_crossed_during_preparation_is_rechecked_before_send() {
    let mock = Mock::new(vec![Ok(json!({"msgs":[]}))]);
    let calls = std::cell::Cell::new(0);
    let clock = || {
        let call = calls.get();
        calls.set(call + 1);
        if call == 0 {
            NOW - 1
        } else {
            NOW
        }
    };
    let result = run_with_clock(&mock, request_with(Some(known(NOW - DAY))), &clock)
        .await
        .unwrap();
    failure(&result, NotificationFailureCode::SubscriptionExpired);
    assert_eq!(mock.requests.borrow().len(), 1);
}
