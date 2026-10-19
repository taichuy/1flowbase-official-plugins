//! GitHub protocol only. Host owns encrypted config, identity binding and sessions.
mod config;
mod eligibility;
mod remote;

use config::Config;
use runtime_extension_sdk::{AuthenticationRequest, AuthenticationResponse, MultiplexEmitter};
use serde::Deserialize;
use serde_json::{json, Value};
use url::Url;

#[derive(Debug)]
pub(crate) struct Failure(&'static str);
pub(crate) type Result<T> = std::result::Result<T, Failure>;

#[derive(Deserialize)]
struct Invocation {
    method: String,
    input: InvocationInput,
}
#[derive(Deserialize)]
struct InvocationInput {
    handler: String,
    config_payload: Value,
    input_payload: Value,
}

pub async fn dispatch(request: Value, emitter: MultiplexEmitter) -> Value {
    let result = async {
        let request: Invocation =
            serde_json::from_value(request).map_err(|_| Failure("github_invocation_invalid"))?;
        if request.method != "execute" || request.input.handler != "authenticate" {
            return Err(Failure("github_operation_unsupported"));
        }
        let config: Config = serde_json::from_value(request.input.config_payload)
            .map_err(|_| Failure("github_config_invalid"))?;
        config.validate()?;
        let operation: AuthenticationRequest = serde_json::from_value(request.input.input_payload)
            .map_err(|_| Failure("github_request_invalid"))?;
        execute(&config, operation, emitter).await
    }
    .await;
    match result {
        Ok(response) => json!({"ok":true,"result":response}),
        // Never forward provider response bodies, Host diagnostics, serde errors or credentials.
        Err(Failure(code)) => json!({"ok":false,"error":{"message":code}}),
    }
}

async fn execute(
    config: &Config,
    request: AuthenticationRequest,
    emitter: MultiplexEmitter,
) -> Result<AuthenticationResponse> {
    match request {
        AuthenticationRequest::Begin {
            callback_url,
            state,
            code_challenge,
        } => {
            config.validate_callback(&callback_url)?;
            if state.is_empty()
                || code_challenge.len() != 43
                || !code_challenge
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            {
                return Err(Failure("github_request_invalid"));
            }
            let mut url = Url::parse("https://github.com/login/oauth/authorize")
                .map_err(|_| Failure("github_request_invalid"))?;
            url.query_pairs_mut()
                .append_pair("client_id", &config.client_id)
                .append_pair("redirect_uri", &callback_url)
                .append_pair("state", &state)
                .append_pair("scope", "user:email")
                .append_pair("code_challenge", &code_challenge)
                .append_pair("code_challenge_method", "S256");
            Ok(AuthenticationResponse::Authorization {
                authorization_url: url.into(),
            })
        }
        AuthenticationRequest::Complete {
            callback_url,
            code,
            code_verifier,
        } => {
            config.validate_callback(&callback_url)?;
            if code.is_empty()
                || !(43..=128).contains(&code_verifier.len())
                || !code_verifier
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
            {
                return Err(Failure("github_request_invalid"));
            }
            remote::complete(config, &code, &code_verifier, emitter).await
        }
        AuthenticationRequest::RegistrationEligibility { profile, now_unix } => {
            Ok(eligibility::evaluate(config, &profile, now_unix))
        }
    }
}

#[cfg(test)]
#[path = "_tests/behavior.rs"]
mod behavior_tests;
