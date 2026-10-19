use base64::{engine::general_purpose::STANDARD, Engine};
use runtime_extension_sdk::{
    serve_io, MultiplexEnvelope, MultiplexHostMessage, MultiplexHostService, MultiplexWorkerMessage,
};
use serde_json::{json, Value};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const CALLBACK: &str = "https://flow.example/api/public/auth/external/callback";
const SECRET: &str = "fixture-client-secret";
const CODE: &str = "fixture-private-code";
const TOKEN: &str = "fixture-access-token";
fn config() -> Value {
    json!({"client_id":"fixture-client-id","client_secret":SECRET,"callback_url":CALLBACK,
        "self_registration_enabled":true,"minimum_account_age_years":0})
}
fn complete() -> Value {
    json!({"operation":"complete","callback_url":CALLBACK,"code":CODE,"code_verifier":"v".repeat(43)})
}
fn http(value: Value) -> Value {
    json!({"result":{"status":200,"headers":{"content-type":"application/json"},
        "body_base64":STANDARD.encode(serde_json::to_vec(&value).unwrap())}})
}
async fn invoke<F>(config: Value, operation: Value, mut callback: F) -> Value
where
    F: FnMut(MultiplexHostService, Value) -> Value,
{
    tokio::task::LocalSet::new()
        .run_until(async {
            let (host, worker) = tokio::io::duplex(8192);
            let (worker_read, worker_write) = tokio::io::split(worker);
            let runner =
                tokio::task::spawn_local(serve_io(worker_read, worker_write, crate::dispatch));
            let (host_read, mut host_write) = tokio::io::split(host);
            let mut reader = BufReader::new(host_read);
            let frame = MultiplexEnvelope::new(MultiplexHostMessage::Call {
                call_id: "fixture".into(),
                request: json!({"method":"execute","input":{
                "handler":"authenticate","config_payload":config,"input_payload":operation}}),
            });
            host_write
                .write_all(format!("{}\n", serde_json::to_string(&frame).unwrap()).as_bytes())
                .await
                .unwrap();
            let response = loop {
                let mut line = String::new();
                assert!(reader.read_line(&mut line).await.unwrap() > 0);
                let frame: MultiplexEnvelope<MultiplexWorkerMessage> =
                    serde_json::from_str(&line).unwrap();
                match frame.message {
                    MultiplexWorkerMessage::Callback {
                        call_id,
                        callback_id,
                        service,
                        request,
                    } => {
                        assert!(request.get("actor_id").is_none());
                        let frame = MultiplexEnvelope::new(MultiplexHostMessage::CallbackResult {
                            call_id,
                            callback_id,
                            response: callback(service, request),
                        });
                        host_write
                            .write_all(
                                format!("{}\n", serde_json::to_string(&frame).unwrap()).as_bytes(),
                            )
                            .await
                            .unwrap();
                    }
                    MultiplexWorkerMessage::Response { response, .. } => break response,
                    other => panic!("unexpected fixture frame: {other:?}"),
                }
            };
            host_write.shutdown().await.unwrap();
            runner.await.unwrap().unwrap();
            response
        })
        .await
}
fn no_http(_: MultiplexHostService, _: Value) -> Value {
    panic!("unexpected network callback")
}
fn assert_safe(value: &Value) {
    for private in [SECRET, CODE, TOKEN, &"v".repeat(43)] {
        assert!(!value.to_string().contains(private));
    }
}

#[tokio::test]
async fn begin_uses_fixed_github_origin_state_callback_pkce_and_minimal_scope() {
    let response = invoke(
        config(),
        json!({"operation":"begin","callback_url":CALLBACK,
        "state":"state with & separators","code_challenge":"c".repeat(43)}),
        no_http,
    )
    .await;
    assert_eq!(response["ok"], true);
    assert_eq!(response["result"]["type"], "authorization");
    let url = url::Url::parse(response["result"]["authorization_url"].as_str().unwrap()).unwrap();
    assert_eq!(url.origin().ascii_serialization(), "https://github.com");
    assert_eq!(url.path(), "/login/oauth/authorize");
    let query: std::collections::BTreeMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(query["client_id"], "fixture-client-id");
    assert_eq!(query["redirect_uri"], CALLBACK);
    assert_eq!(query["state"], "state with & separators");
    assert_eq!(query["scope"], "user:email");
    assert_eq!(query["code_challenge"], "c".repeat(43));
    assert_eq!(query["code_challenge_method"], "S256");
    assert_eq!(query.len(), 6);
    assert_safe(&response);
}

async fn identity(login: &str, emails: Value) -> Value {
    let mut calls = 0;
    let response = invoke(config(), complete(), |service, request| {
        assert!(matches!(service, MultiplexHostService::HttpEgressV1));
        calls += 1;
        assert_eq!(request["headers"]["accept"], "application/json");
        match calls {
            1 => {
                assert_eq!(request["method"], "POST");
                assert_eq!(request["url"], "https://github.com/login/oauth/access_token");
                assert_eq!(request["headers"]["content-type"], "application/x-www-form-urlencoded");
                let body = STANDARD.decode(request["body_base64"].as_str().unwrap()).unwrap();
                let pairs: std::collections::BTreeMap<_, _> = url::form_urlencoded::parse(&body).into_owned().collect();
                assert_eq!(pairs["client_secret"], SECRET);
                assert_eq!(pairs["client_id"], "fixture-client-id");
                assert_eq!(pairs["redirect_uri"], CALLBACK);
                assert_eq!(pairs["code"], CODE);
                assert_eq!(pairs["code_verifier"], "v".repeat(43));
                http(json!({"access_token":TOKEN,"token_type":"bearer","scope":"user:email"}))
            }
            2 => {
                assert_eq!(request["method"], "GET");
                assert_eq!(request["url"], "https://api.github.com/user");
                assert_eq!(request["headers"]["authorization"], format!("Bearer {TOKEN}"));
                http(json!({"id":12345678901234u64,"login":login,"name":"Fixture User",
                    "avatar_url":"https://avatars.githubusercontent.com/u/12345678901234",
                    "created_at":"2020-02-29T23:59:59Z","email":"unverified-user-field@example.test",
                    "access_token":TOKEN,"code":CODE,"client_secret":SECRET,"private_gists":99}))
            }
            3 => {
                assert_eq!(request["method"], "GET");
                assert_eq!(request["url"], "https://api.github.com/user/emails");
                assert_eq!(request["headers"]["authorization"], format!("Bearer {TOKEN}"));
                http(emails.clone())
            }
            _ => panic!("extra request"),
        }
    }).await;
    assert_eq!(calls, 3);
    assert_safe(&response);
    response
}

#[tokio::test]
async fn exchange_uses_host_http_and_stable_numeric_identity_with_verified_primary_email_only() {
    let emails = json!([
        {"email":"secondary@example.test","primary":false,"verified":true},
        {"email":"unverified@example.test","primary":true,"verified":false},
        {"email":"verified-primary@example.test","primary":true,"verified":true}]);
    let first = identity("old-name", emails.clone()).await;
    let renamed = identity("new-name", emails).await;
    assert_eq!(first["ok"], true);
    assert_eq!(first["result"]["type"], "verified_identity");
    assert_eq!(first["result"]["subject_type"], "github_user_id");
    assert_eq!(first["result"]["subject_value"], "12345678901234");
    assert_eq!(
        first["result"]["subject_value"],
        renamed["result"]["subject_value"]
    );
    assert_eq!(first["result"]["issuer"], "https://github.com");
    assert!(first["result"]["realm"].is_null());
    assert_eq!(
        first["result"]["profile"],
        json!({"login":"old-name","name":"Fixture User",
        "avatar_url":"https://avatars.githubusercontent.com/u/12345678901234",
        "created_at":"2020-02-29T23:59:59Z","email":"verified-primary@example.test","email_verified":true})
    );
    assert_eq!(renamed["result"]["profile"]["login"], "new-name");
}

#[tokio::test]
async fn no_verified_primary_email_still_returns_identity_for_existing_bindings() {
    for emails in [
        json!([]),
        json!([
        {"email":"secondary@example.test","primary":false,"verified":true},
        {"email":"unverified@example.test","primary":true,"verified":false}]),
    ] {
        let response = identity("fixture", emails).await;
        assert_eq!(response["ok"], true);
        assert!(response["result"]["profile"].get("email").is_none());
        assert_eq!(response["result"]["profile"]["email_verified"], false);
    }
}

#[tokio::test]
async fn errors_never_echo_private_provider_or_host_diagnostics() {
    for failure in [
        json!({"error":{"code":format!("{SECRET}-{TOKEN}"),"details":{"code":CODE}}}),
        json!({"result":{"status":401,"headers":{},"body_base64":STANDARD.encode(format!("{SECRET}-{CODE}-{TOKEN}"))}}),
        http(
            json!({"error":"bad_verification_code","error_description":format!("{SECRET}-{CODE}-{TOKEN}")}),
        ),
        http(json!({"access_token":null,"error_description":SECRET})),
    ] {
        let response = invoke(config(), complete(), |_, _| failure.clone()).await;
        assert_eq!(response["ok"], false);
        assert_safe(&response);
        assert!(response["error"]["message"]
            .as_str()
            .unwrap()
            .starts_with("github_"));
        assert_eq!(response["error"].as_object().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn callback_mismatch_and_invalid_config_stop_before_network() {
    let mut operation = complete();
    operation["callback_url"] = json!("https://evil.example/callback");
    assert_eq!(
        invoke(config(), operation, no_http).await["error"]["message"],
        "github_callback_mismatch"
    );
    for minimum in [json!(-1), json!(1.5), json!("2")] {
        let mut invalid = config();
        invalid["minimum_account_age_years"] = minimum;
        let response = invoke(invalid, complete(), no_http).await;
        assert_eq!(response["error"]["message"], "github_config_invalid");
        assert_safe(&response);
    }
}

fn unix(value: &str) -> i64 {
    OffsetDateTime::parse(value, &Rfc3339)
        .unwrap()
        .unix_timestamp()
}
#[tokio::test]
async fn registration_uses_utc_calendar_anniversaries_and_february_28_leap_policy() {
    for (created, minimum, now, expected) in [
        ("2026-10-19T00:00:00Z", 0, "2026-10-19T00:00:00Z", true),
        ("2025-10-20T00:00:00Z", 1, "2026-10-19T23:59:59Z", false),
        ("2025-10-19T23:59:59Z", 1, "2026-10-19T00:00:00Z", true),
        ("2020-02-29T12:00:00Z", 1, "2021-02-27T23:59:59Z", false),
        ("2020-02-29T12:00:00Z", 1, "2021-02-28T00:00:00Z", true),
        ("2020-02-29T12:00:00Z", 4, "2024-02-28T23:59:59Z", false),
        ("2020-02-29T12:00:00Z", 4, "2024-02-29T00:00:00Z", true),
        ("2025-10-20T00:30:00+01:00", 1, "2026-10-19T00:00:00Z", true),
        ("2027-01-01T00:00:00Z", 0, "2026-10-19T00:00:00Z", false),
        ("invalid", 0, "2026-10-19T00:00:00Z", false),
        ("", 2, "2026-10-19T00:00:00Z", false),
    ] {
        let mut config = config();
        config["minimum_account_age_years"] = json!(minimum);
        let response = invoke(
            config,
            json!({"operation":"registration_eligibility",
            "profile":{"created_at":created},"now_unix":unix(now)}),
            no_http,
        )
        .await;
        assert_eq!(response["ok"], true);
        assert_eq!(response["result"]["type"], "registration_eligibility");
        assert_eq!(
            response["result"]["allowed"], expected,
            "{created}, {minimum}, {now}"
        );
        assert_eq!(response["result"]["reason_code"].is_null(), expected);
    }
    let mut disabled = config();
    disabled["self_registration_enabled"] = json!(false);
    let response = invoke(
        disabled,
        json!({"operation":"registration_eligibility",
        "profile":{"created_at":"2008-01-01T00:00:00Z"},"now_unix":unix("2026-10-19T00:00:00Z")}),
        no_http,
    )
    .await;
    assert_eq!(response["result"]["allowed"], false);
    assert_eq!(
        response["result"]["reason_code"],
        "github_registration_disabled"
    );
}

#[tokio::test]
async fn identity_rejects_missing_string_or_nonpositive_numeric_ids() {
    for id in [json!(null), json!("123"), json!(0), json!(-1), json!(1.5)] {
        let mut calls = 0;
        let response = invoke(config(), complete(), |_, _| {
            calls += 1;
            if calls == 1 {
                http(json!({"access_token":TOKEN}))
            } else {
                http(json!({"id":id,"login":"fixture","client_secret":SECRET}))
            }
        })
        .await;
        assert_eq!(calls, 2);
        assert_eq!(response["ok"], false);
        assert_eq!(response["error"]["message"], "github_identity_invalid");
        assert_safe(&response);
    }
}
