use crate::{build_chat_completion_body, ProviderInvocationInput};
use serde_json::json;

fn provider_input(network_egress: serde_json::Value) -> ProviderInvocationInput {
    serde_json::from_value(json!({
        "contract_version": "1flowbase.provider/v2",
        "provider_instance_id": "provider-test",
        "provider_code": "deepseek",
        "protocol": "openai_compatible",
        "model": "deepseek-v4-flash",
        "provider_config": {
            "base_url": "https://api.deepseek.com",
            "api_key": "test-key",
            "proxy_url": "http://configured-proxy.invalid:8080"
        },
        "messages": [{"role": "user", "content": "hello"}],
        "required_capabilities": ["network_egress_handoff/v1"],
        "run_context": {"network_egress": network_egress}
    }))
    .expect("fixture should satisfy provider input contract")
}

#[test]
fn root_1805_network_egress_handoff_is_required_and_does_not_reject_the_invocation() {
    let input = provider_input(json!({
        "mode": "required_http_proxy",
        "http_proxy_url": "http://host-egress.invalid:3128",
        "expires_at": "2026-08-21T04:00:00Z",
        "required": true
    }));

    assert_eq!(
        input
            .required_network_egress_proxy_url()
            .unwrap()
            .as_deref(),
        Some("http://host-egress.invalid:3128")
    );
    assert!(build_chat_completion_body(&input).is_ok());
}

#[test]
fn root_1805_network_egress_handoff_fails_closed_without_required_proxy_context() {
    let input = provider_input(json!({
        "mode": "required_http_proxy",
        "http_proxy_url": "https://not-an-http-proxy.invalid",
        "expires_at": "2026-08-21T04:00:00Z",
        "required": true
    }));

    assert!(input.required_network_egress_proxy_url().is_err());
}
