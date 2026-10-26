use super::*;
use runtime_extension_sdk::{NotificationAttachmentDownloadOutcome, NotificationReceiveOutcome};
const NOW: u64 = 1_800_000_000_000;
fn receive_input(cursor: Option<Value>, max_messages: u32) -> NotificationRuntimeInput {
    input(
        NotificationOperation::Receive {
            cursor,
            max_messages,
            wait_seconds: 20,
        },
        secrets(),
    )
}
fn text_message(id: &str, text: &str) -> Value {
    json!({"message_id":id,"from_user_id":"scanner","message_type":1,"create_time_ms":NOW-1000,"context_token":"reply-context","item_list":[{"type":1,"text_item":{"text":text}}]})
}
fn batch(result: &NotificationRuntimeResult) -> &runtime_extension_sdk::NotificationReceiveBatch {
    match result {
        NotificationRuntimeResult::Receive {
            outcome: NotificationReceiveOutcome::Succeeded { batch },
            ..
        } => batch,
        _ => panic!("expected successful receive"),
    }
}
#[tokio::test]
async fn receive_retains_overflow_and_only_host_owns_cursor_commit() {
    let mock = Mock::new(vec![Ok(
        json!({"msgs":[text_message("a","first"),text_message("b","second"),text_message("c","third")],"get_updates_buf":"next"}),
    )]);
    let first = run_with_clock(&mock, receive_input(None, 1), &|| NOW)
        .await
        .unwrap();
    assert_eq!(batch(&first).messages[0].text.as_deref(), Some("first"));
    assert!(batch(&first).has_more);
    let second = run_with_clock(
        &mock,
        receive_input(batch(&first).candidate_cursor.clone(), 2),
        &|| NOW,
    )
    .await
    .unwrap();
    assert_eq!(batch(&second).messages.len(), 2);
    assert_eq!(batch(&second).messages[1].provider_message_id, "c");
    assert_eq!(batch(&second).candidate_cursor, Some(json!("next")));
    assert!(!batch(&second).has_more);
    assert_eq!(mock.requests.borrow().len(), 1);
    assert_eq!(
        mock.requests.borrow()[0].body.as_ref().unwrap()["get_updates_buf"],
        ""
    );
    let mut stored = secrets();
    apply(&mut stored, &first);
    assert!(!stored.contains_key("cursor"));
    assert_eq!(stored["context_token"], "reply-context");
    first
        .validate_for(&receive_input(None, 1), &descriptor())
        .unwrap();
}
#[tokio::test]
async fn receive_deduplicates_in_batch_and_replays_without_committed_cursor() {
    let response = json!({"ret":0,"msgs":[text_message("a","hello"),text_message("a","hello")],"get_updates_buf":"opaque"});
    let mock = Mock::new(vec![Ok(response.clone()), Ok(response)]);
    for _ in 0..2 {
        let result = run_with_clock(&mock, receive_input(Some(json!("previous")), 100), &|| NOW)
            .await
            .unwrap();
        assert_eq!(batch(&result).messages.len(), 1);
        assert_eq!(batch(&result).candidate_cursor, Some(json!("opaque")));
    }
    assert_eq!(
        mock.requests.borrow()[1].body.as_ref().unwrap()["get_updates_buf"],
        "previous"
    );
}
#[tokio::test]
async fn receive_filters_other_senders_and_bot_echoes_and_preserves_private_scope() {
    let mut other = text_message("other", "private");
    other["from_user_id"] = json!("attacker");
    let mut echo = text_message("echo", "echo");
    echo["message_type"] = json!(2);
    let mock = Mock::new(vec![Ok(
        json!({"msgs":[other,echo,text_message("valid","你好")],"get_updates_buf":"c"}),
    )]);
    let result = run_with_clock(&mock, receive_input(None, 100), &|| NOW)
        .await
        .unwrap();
    assert_eq!(batch(&result).messages.len(), 1);
    assert_eq!(batch(&result).messages[0].sender.id, "scanner");
    assert_eq!(batch(&result).messages[0].conversation_id, "scanner");
    let mut stored = secrets();
    apply(&mut stored, &result);
    assert_eq!(stored["bound_user_id"], "scanner");
}
#[tokio::test]
async fn empty_poll_commits_candidate_but_malformed_batch_cannot_advance() {
    let mock = Mock::new(vec![
        Ok(json!({"get_updates_buf":"next"})),
        Ok(json!({"msgs":"invalid","get_updates_buf":"lost"})),
        Ok(
            json!({"msgs":[{"from_user_id":"scanner","message_type":1,"item_list":[{"type":1,"text_item":{"text":"missing id"}}]}],"get_updates_buf":"lost"}),
        ),
    ]);
    let result = run_with_clock(&mock, receive_input(None, 100), &|| NOW)
        .await
        .unwrap();
    assert!(batch(&result).messages.is_empty());
    assert_eq!(batch(&result).candidate_cursor, Some(json!("next")));
    assert!(run_with_clock(&mock, receive_input(None, 100), &|| NOW)
        .await
        .is_err());
    assert!(run_with_clock(&mock, receive_input(None, 100), &|| NOW)
        .await
        .is_err());
}
#[tokio::test]
async fn receive_invalid_auth_revokes_and_network_timeout_never_returns_candidate() {
    let mock = Mock::new(vec![Ok(json!({"ret":-14})), Err(ProviderError::Timeout)]);
    let result = run_with_clock(&mock, receive_input(None, 100), &|| NOW)
        .await
        .unwrap();
    assert!(matches!(
        result,
        NotificationRuntimeResult::Receive {
            outcome: NotificationReceiveOutcome::Failed {
                failure_code: NotificationFailureCode::AuthorizationInvalid
            },
            ..
        }
    ));
    let mut stored = secrets();
    apply(&mut stored, &result);
    assert!(!stored.contains_key("bot_token"));
    assert!(!stored.contains_key("cursor"));
    assert!(run_with_clock(&mock, receive_input(None, 100), &|| NOW)
        .await
        .is_err());
}
#[tokio::test]
async fn maintain_and_send_never_consume_updates_and_send_uses_received_context() {
    let mut stored = secrets();
    stored.insert("context_token".into(), json!("received-context"));
    let mock = Mock::new(vec![Ok(json!({"message_id":"accepted"}))]);
    let maintained = run(
        &mock,
        auth(NotificationAuthorizationOperation::Maintain, stored.clone()),
    )
    .await
    .unwrap();
    assert_eq!(
        view(&maintained),
        NotificationAuthorizationStatus::Authorized
    );
    assert!(mock.requests.borrow().is_empty());
    let result = run(&mock, send(stored)).await.unwrap();
    assert!(matches!(
        result,
        NotificationRuntimeResult::Send {
            outcome: NotificationSendOutcome::Succeeded { .. },
            ..
        }
    ));
    let requests = mock.requests.borrow();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].endpoint, "ilink/bot/sendmessage");
    let msg = &requests[0].body.as_ref().unwrap()["msg"];
    assert_eq!(msg["context_token"], "received-context");
    assert_eq!(msg["to_user_id"], "scanner");
    assert_eq!(msg["item_list"][0]["text_item"]["text"], "你好");
}
#[tokio::test]
async fn send_classifies_rejection_and_ambiguity_without_retry() {
    for (response, expected) in [
        (
            Ok(json!({"ret":-14})),
            Some(NotificationFailureCode::AuthorizationInvalid),
        ),
        (
            Ok(json!({"ret":-2})),
            Some(NotificationFailureCode::RecipientNotReady),
        ),
        (
            Ok(json!({"ret":42})),
            Some(NotificationFailureCode::ProviderRejected),
        ),
        (Err(ProviderError::Network), None),
        (Ok(json!({"message_id":"ack","ret":"0"})), None),
    ] {
        let mock = Mock::new(vec![response]);
        let result = run(&mock, send(secrets())).await.unwrap();
        if let Some(expected) = expected {
            failure(&result, expected);
        } else {
            assert!(matches!(
                result,
                NotificationRuntimeResult::Send {
                    outcome: NotificationSendOutcome::Unknown { .. },
                    ..
                }
            ));
        }
        assert_eq!(mock.requests.borrow().len(), 1);
        result
            .validate_for(&send(secrets()), &descriptor())
            .unwrap();
    }
}
#[tokio::test]
async fn expired_reply_window_blocks_send_without_provider_io() {
    let mock = Mock::new(vec![]);
    let mut request = send(secrets());
    request
        .delivery_window
        .as_mut()
        .unwrap()
        .last_received_at_ms = Some(1);
    let result = run(&mock, request).await.unwrap();
    failure(&result, NotificationFailureCode::SubscriptionExpired);
    assert!(mock.requests.borrow().is_empty());
}
fn media_item() -> Value {
    json!({"type":4,"file_item":{"file_name":"report.txt","len":"5","media":{"encrypt_query_param":"opaque-query","aes_key":STANDARD.encode(b"0123456789abcdef")}}})
}
fn encrypt(plain: &[u8]) -> Vec<u8> {
    use aws_lc_rs::cipher::{PaddedBlockEncryptingKey, UnboundCipherKey, AES_128};
    let key = UnboundCipherKey::new(&AES_128, b"0123456789abcdef").unwrap();
    let key = PaddedBlockEncryptingKey::ecb_pkcs7(key).unwrap();
    let mut data = plain.to_vec();
    key.encrypt(&mut data).unwrap();
    data
}
#[tokio::test]
async fn file_message_produces_private_reference_and_download_decrypts_exact_bytes() {
    let mut message = text_message("file-message", "caption");
    message["item_list"]
        .as_array_mut()
        .unwrap()
        .push(media_item());
    let mock = Mock::new(vec![
        Ok(json!({"msgs":[message],"get_updates_buf":"after-file"})),
        Ok(json!({"data":STANDARD.encode(encrypt(b"hello")),"eof":true})),
    ]);
    let received = run_with_clock(&mock, receive_input(None, 100), &|| NOW)
        .await
        .unwrap();
    let message = &batch(&received).messages[0];
    assert_eq!(message.text.as_deref(), Some("caption"));
    let file = &message.attachments[0];
    assert_eq!(file.file_name, "report.txt");
    assert_eq!(file.size_bytes, Some(5));
    assert_eq!(file.reference.provider_message_id, "file-message");
    let request = input(
        NotificationOperation::DownloadAttachment {
            attachment: file.reference.clone(),
            offset: 0,
            max_bytes: 49152,
        },
        secrets(),
    );
    let result = run(&mock, request.clone()).await.unwrap();
    result.validate_for(&request, &descriptor()).unwrap();
    assert!(
        matches!(result,NotificationRuntimeResult::DownloadAttachment{outcome:NotificationAttachmentDownloadOutcome::Succeeded{offset:0,data_base64,eof:true}} if STANDARD.decode(&data_base64).unwrap()==b"hello")
    );
    let requests = mock.requests.borrow();
    let download = &requests[1];
    assert!(download.token.is_none());
    assert_eq!(download.base_url, "https://novac2c.cdn.weixin.qq.com");
    assert_eq!(download.endpoint, "c2c/download");
}
#[tokio::test]
async fn large_file_chunks_have_no_total_cap_and_support_unaligned_offsets() {
    let plain: Vec<u8> = (0..(160 * 1024 + 3)).map(|i| (i % 251) as u8).collect();
    let ciphertext = encrypt(&plain);
    let mut file = media::attachment(&media_item(), "large", 0)
        .unwrap()
        .unwrap();
    file.reference.download_token["size_bytes"] = json!(plain.len());
    let mut recovered = Vec::new();
    let mut offset = 0;
    loop {
        let start = offset / 16 * 16;
        let end = (offset + 49152 + 31) / 16 * 16;
        let end = end.min(ciphertext.len());
        let mock = Mock::new(vec![Ok(
            json!({"data":STANDARD.encode(&ciphertext[start..end]),"eof":end==ciphertext.len()}),
        )]);
        let request = input(
            NotificationOperation::DownloadAttachment {
                attachment: file.reference.clone(),
                offset: offset as u64,
                max_bytes: 49152,
            },
            secrets(),
        );
        let result = run(&mock, request.clone()).await.unwrap();
        result.validate_for(&request, &descriptor()).unwrap();
        match result {
            NotificationRuntimeResult::DownloadAttachment {
                outcome:
                    NotificationAttachmentDownloadOutcome::Succeeded {
                        data_base64, eof, ..
                    },
            } => {
                let data = STANDARD.decode(data_base64).unwrap();
                assert!(data.len() <= 49152);
                offset += data.len();
                recovered.extend(data);
                if eof {
                    break;
                }
            }
            _ => panic!("download failed"),
        }
    }
    assert_eq!(recovered, plain);
    let mock = Mock::new(vec![Ok(
        json!({"data":STANDARD.encode(&ciphertext[..48]),"eof":false}),
    )]);
    let result = media::download(&mock, &secrets(), file.reference, 3, 17)
        .await
        .unwrap();
    assert!(
        matches!(result,NotificationRuntimeResult::DownloadAttachment{outcome:NotificationAttachmentDownloadOutcome::Succeeded{data_base64,eof:false,..}} if STANDARD.decode(&data_base64).unwrap()==plain[3..20])
    );
}
#[tokio::test]
async fn media_rejects_unsafe_locator_bad_padding_size_and_chunk_without_progress() {
    for media in [
        json!({"full_url":"https://evil.com/c2c/download?encrypted_query_param=x"}),
        json!({"full_url":"https://novac2c.cdn.weixin.qq.com/other?encrypted_query_param=x"}),
        json!({"full_url":"https://user@novac2c.cdn.weixin.qq.com/c2c/download?encrypted_query_param=x"}),
    ] {
        let mut item = media_item();
        item["file_item"]["media"] = media;
        assert!(media::attachment(&item, "id", 0).is_err());
    }
    for (data, eof) in [
        (vec![0u8; 15], true),
        (vec![0u8; 16], true),
        (vec![], false),
        (encrypt(b"wrong-size"), true),
    ] {
        let file = media::attachment(&media_item(), "id", 0).unwrap().unwrap();
        let mock = Mock::new(vec![Ok(json!({"data":STANDARD.encode(data),"eof":eof}))]);
        assert!(media::download(&mock, &secrets(), file.reference, 0, 49152)
            .await
            .is_err());
    }
}
#[test]
fn receive_descriptor_declares_progressive_host_tools_and_no_secret_cursor() {
    let descriptor = descriptor();
    descriptor.validate().unwrap();
    let receive = descriptor.receive.unwrap();
    assert!(receive.default_system_prompt.contains("final reply"));
    assert_eq!(receive.mcp_tools.len(), 3);
    for instruction in [
        "file message.id as message_id",
        "at most 5 real messages",
        "default limit of 5",
        "newer to older messages",
        "next_cursor unchanged as cursor",
        "type (text or file)",
        "direction (inbound or outbound)",
    ] {
        assert!(receive.default_system_prompt.contains(instruction));
    }
    assert!(!receive.default_system_prompt.contains("attachment_id"));
    for tool in &receive.mcp_tools {
        assert!(!tool.input_schema.to_string().contains("attachment_id"));
    }
    let read_file = receive
        .mcp_tools
        .iter()
        .find(|tool| tool.name == "read_file")
        .unwrap();
    assert_eq!(read_file.input_schema["required"], json!(["message_id"]));
    assert_eq!(
        read_file.input_schema["properties"]["message_id"]["format"],
        "uuid"
    );
    assert_eq!(read_file.input_schema["additionalProperties"], false);
    for tool in receive
        .mcp_tools
        .iter()
        .filter(|tool| tool.name.starts_with("history_"))
    {
        let properties = &tool.input_schema["properties"];
        assert_eq!(properties["limit"]["minimum"], 1);
        assert_eq!(properties["limit"]["maximum"], 5);
        assert_eq!(properties["limit"]["default"], 5);
        assert!(properties["cursor"]["description"]
            .as_str()
            .unwrap()
            .contains("next earlier page of real messages"));
        assert_eq!(
            tool.input_schema["properties"]["conversation_id"]["format"],
            "uuid"
        );
        assert!(
            tool.input_schema["properties"]["conversation_id"]["description"]
                .as_str()
                .unwrap()
                .contains("never external_conversation_key")
        );
    }
    assert!(descriptor
        .authorization
        .unwrap()
        .managed_secret_keys
        .iter()
        .all(|key| key != "cursor"));
}

#[tokio::test]
async fn targeted_reply_uses_explicit_matching_private_peer() {
    let mut stored = secrets();
    stored.insert(
        "context_token".into(),
        json!("opaque-latest-context-not-a-destination"),
    );
    let mut request = send(stored);
    let NotificationOperation::Send { target, .. } = &mut request.operation else {
        unreachable!()
    };
    *target = Some(NotificationSendTarget {
        sender_id: "scanner".into(),
        conversation_id: "scanner".into(),
        provider_message_id: "durable-inbound-message".into(),
    });
    let mock = Mock::new(vec![Ok(json!({"message_id":"accepted-targeted-reply"}))]);
    let result = run(&mock, request.clone()).await.unwrap();
    result.validate_for(&request, &descriptor()).unwrap();
    assert!(matches!(
        result,
        NotificationRuntimeResult::Send {
            outcome: NotificationSendOutcome::Succeeded { .. },
            ..
        }
    ));
    let requests = mock.requests.borrow();
    assert_eq!(requests.len(), 1);
    let msg = &requests[0].body.as_ref().unwrap()["msg"];
    assert_eq!(msg["to_user_id"], "scanner");
    assert_eq!(
        msg["context_token"],
        "opaque-latest-context-not-a-destination"
    );
}

#[tokio::test]
async fn targeted_reply_rejects_wrong_sender_or_conversation_before_provider_io() {
    for (sender_id, conversation_id) in [
        ("attacker", "scanner"),
        ("scanner", "other-conversation"),
        ("attacker", "attacker"),
    ] {
        let mock = Mock::new(vec![]);
        let mut request = send(secrets());
        let NotificationOperation::Send { target, .. } = &mut request.operation else {
            unreachable!()
        };
        *target = Some(NotificationSendTarget {
            sender_id: sender_id.into(),
            conversation_id: conversation_id.into(),
            provider_message_id: "durable-inbound-message".into(),
        });
        let result = run(&mock, request.clone()).await.unwrap();
        failure(&result, NotificationFailureCode::ProviderRejected);
        assert_eq!(view(&result), NotificationAuthorizationStatus::Authorized);
        assert!(mock.requests.borrow().is_empty());
        result.validate_for(&request, &descriptor()).unwrap();
    }
}
