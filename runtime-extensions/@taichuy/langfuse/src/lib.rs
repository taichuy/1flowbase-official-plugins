//! Vendor behavior lives here; trusted identity, storage and networking belong to the Host.
mod config;
pub mod mapping;
mod openapi;
mod operations;
mod remote;

use runtime_extension_sdk::{
    ApplicationLogsClient, ApplicationLogsClientError, ApplicationLogsRequest,
    ApplicationLogsResponse, HttpEgressClient, MultiplexEmitter,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize)]
pub struct Failure {
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}
pub type Result<T> = std::result::Result<T, Failure>;
pub fn fail(code: &str) -> Failure {
    Failure {
        code: code.into(),
        status: None,
        details: None,
    }
}
impl From<ApplicationLogsClientError> for Failure {
    fn from(error: ApplicationLogsClientError) -> Self {
        match error {
            ApplicationLogsClientError::Host(error) => Self {
                code: error.code,
                status: error.status,
                details: error.details,
            },
            ApplicationLogsClientError::Transport(
                runtime_extension_sdk::MultiplexError::FrameTooLarge,
            ) => fail("host_application_logs_frame_too_large"),
            _ => fail("host_application_logs_transport"),
        }
    }
}
pub(crate) struct Host {
    emitter: MultiplexEmitter,
}
impl Host {
    async fn logs(&self, request: ApplicationLogsRequest) -> Result<ApplicationLogsResponse> {
        ApplicationLogsClient::new(self.emitter.clone())
            .execute(request)
            .await
            .map_err(Into::into)
    }
}
#[derive(Deserialize)]
struct Invocation {
    input: InvocationInput,
}
#[derive(Deserialize)]
struct InvocationInput {
    handler: String,
    input_payload: Value,
}

/// Return business failures as safe structured output so already committed receipts survive.
/// The outer response remains the SDK capability envelope; no raw transport error is exposed.
pub async fn dispatch(request: Value, emitter: MultiplexEmitter) -> Value {
    let result = async {
        let request: Invocation =
            serde_json::from_value(request).map_err(|_| fail("invalid_invocation"))?;
        let payload = request.input.input_payload;
        operations::execute(&Host { emitter }, &request.input.handler, payload).await
    }
    .await;
    let output = match result {
        Ok(value) => value,
        Err(error) => json!({"success":false,"error":error}),
    };
    json!({"ok":true,"result":output})
}
#[cfg(test)]
#[path = "_tests/mapping.rs"]
mod mapping_tests;
#[cfg(test)]
#[path = "_tests/remote.rs"]
mod remote_tests;
#[cfg(test)]
#[path = "_tests/workflow.rs"]
mod workflow_tests;

#[cfg(test)]
#[path = "_tests/instances.rs"]
mod instances_tests;

#[cfg(test)]
#[path = "_tests/openapi/mod.rs"]
mod openapi_tests;
