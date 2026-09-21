use super::*;
use serde_json::Value;

#[derive(Clone, Debug)]
enum Event {
    Business(usize),
    Observation(ProtocolObservation),
}
fn observation(event: ProtocolObservation) -> Event {
    Event::Observation(event)
}

#[tokio::test]
async fn saturation_preserves_business_and_reports_loss_without_complete() {
    let mut events = Vec::new();
    capture_with_limits(
        "fixture".into(),
        |event| {
            events.push(event.clone());
            Ok(())
        },
        observation,
        |mut sink| async move {
            for index in 0..1000 {
                record("sse", "received", "response_body", b"data: {}\n\n", None);
                sink(&Event::Business(index))?;
            }
            Ok(())
        },
        Limits {
            messages: 2,
            bytes: 1024,
        },
    )
    .await
    .unwrap();
    let business: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            Event::Business(index) => Some(*index),
            _ => None,
        })
        .collect();
    assert_eq!(business, (0..1000).collect::<Vec<_>>());
    let observations: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            Event::Observation(event) => Some(event),
            _ => None,
        })
        .collect();
    assert_eq!(observations.len(), 3);
    assert!(!observations.iter().any(|event| event.kind == "stream_end"));
    let integrity: Value = serde_json::from_str(&observations[2].body).unwrap();
    assert_eq!(integrity["dropped_count"], 998);
    assert_eq!(integrity["reason"], "observation_capacity_exceeded");
}

#[tokio::test]
async fn encoded_byte_limit_rejects_oversize_before_allocation() {
    let mut events = Vec::new();
    capture_with_limits(
        "fixture".into(),
        |event| {
            events.push(event.clone());
            Ok(())
        },
        observation,
        |_sink| async {
            record("websocket", "received", "message", &[0xff; 400], None);
            Ok(())
        },
        Limits {
            messages: 128,
            bytes: 512,
        },
    )
    .await
    .unwrap();
    assert_eq!(events.len(), 1);
    let Event::Observation(event) = &events[0] else {
        panic!()
    };
    assert_eq!(event.kind, "capture_integrity");
    assert_eq!(
        serde_json::from_str::<Value>(&event.body).unwrap()["dropped_count"],
        1
    );
}

#[tokio::test]
async fn websocket_binary_and_prepared_payload_are_lossless() {
    let mut events = Vec::new();
    capture(
        "openai.responses".into(),
        |event| {
            events.push(event.clone());
            Ok(())
        },
        observation,
        |_sink| async {
            record(
                "websocket",
                "prepared",
                "request_prepared",
                b"{\"type\":\"response.create\"}",
                None,
            );
            record(
                "websocket",
                "received",
                "message",
                &[0xff, 0x00, 0x80],
                None,
            );
            Ok(())
        },
    )
    .await
    .unwrap();
    let observations: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            Event::Observation(event) => Some(event),
            _ => None,
        })
        .collect();
    assert_eq!(observations[0].kind, "request_prepared");
    assert_eq!(observations[0].direction, "prepared");
    assert_eq!(observations[1].encoding, "base64");
    assert_eq!(observations[1].body, "/wCA");
    assert_eq!(observations[2].kind, "stream_end");
}

#[tokio::test]
async fn failed_send_has_prepared_fact_and_no_success_terminal() {
    let mut events = Vec::new();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let result: Result<()> = capture(
        "fixture".into(),
        |event| {
            events.push(event.clone());
            Ok(())
        },
        observation,
        |_sink| async move {
            reqwest::Client::new()
                .post(format!("http://{address}/responses"))
                .body("actual serialized request")
                .send_observed()
                .await?;
            Ok(())
        },
    )
    .await;
    assert!(result.is_err());
    assert_eq!(events.len(), 1);
    let Event::Observation(event) = &events[0] else {
        panic!()
    };
    assert_eq!(event.kind, "request_prepared");
    assert_eq!(event.direction, "prepared");
    assert_eq!(event.body, "actual serialized request");
}

#[tokio::test]
async fn cancelling_capture_never_emits_a_success_terminal() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink_events = events.clone();
    let capture = capture(
        "fixture".into(),
        move |event| {
            sink_events.borrow_mut().push(event.clone());
            Ok(())
        },
        observation,
        |_sink| async {
            record(
                "sse",
                "received",
                "response_body",
                b"data: partial\n\n",
                None,
            );
            std::future::pending::<Result<()>>().await
        },
    );
    tokio::pin!(capture);
    tokio::select! {
        biased;
        _ = &mut capture => panic!("capture should remain pending"),
        _ = tokio::task::yield_now() => {}
    }
    assert!(!events
        .borrow()
        .iter()
        .any(|event| matches!(event, Event::Observation(event) if event.kind == "stream_end")));
}
